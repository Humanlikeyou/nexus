import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import type { TakePage } from "../amaraTake/types";
import { ensureTakeLog, forgetTake, historyView, memoryLineView, takeLogEntries } from "./takeLog";
import { tokyoDay } from "../lib/mindBody";
import { liveSuggestMethods } from "./liveSuggest";

/**
 * design-1111t: take memory, one suggestion a day per page, follow-through and outcomes.
 * Taisun (15:32 JST Nov 11): a new suggestion every day (no 3-week trial); Amara may mark Done from the
 * data, with Undo; takes are kept 1 year, then summarized into Memory; no "Let Home do it"; outcomes are
 * grouped by recommendation type over time with the same honesty rules.
 *
 * In the app every read is unwired ("Needs wiring") until Claude builds the take log, catalog, adherence
 * and outcome stores (docs/design/HANDOFF-1111t.md). The sample adapter is preview/test only and every
 * value it returns carries a "Sample data" tag. Evidence sources are placeholders until a person checks them.
 */

export type EvidenceLevel = "strong" | "moderate" | "early" | "weak" | "own";
export const LEVEL_WORD: Record<EvidenceLevel, string> = { strong: "Strong", moderate: "Moderate", early: "Early", weak: "Weak", own: "Your data only" };
export const LEVEL_BARS: Record<EvidenceLevel, number> = { strong: 4, moderate: 3, early: 2, weak: 1, own: 0 };

/** A catalog citation. `verified` stays false until a person has checked it; the UI marks it as a placeholder. */
export type Source = { cite: string; verified: boolean };
export type Evidence = { level: EvidenceLevel; summary: string; sources: Source[]; risk?: string; alsoConsidered?: string };

export type AnswerState = "waiting" | "done" | "skipped" | "not_today" | "unknown";
export type SkipReason = "no_time" | "didnt_feel_like_it" | "forgot" | "not_useful";
export const SKIP_REASONS: Array<[SkipReason, string]> = [["no_time", "No time"], ["didnt_feel_like_it", "Didn't feel like it"], ["forgot", "Forgot"], ["not_useful", "Not useful"]];
export type Answer = { state: AnswerState; source?: "tap" | "detected"; at?: string; detail?: string; reason?: SkipReason | null };

export type Fact = { label: string; value: string; note: string };

/** Today's one thing on a page. `detectRule` null = tap only (detection not wired or not possible). */
export type Suggestion = {
  id: string; page: TakePage; type: string; typeLabel: string; title: string; how: string; why: string; facts: Fact[];
  evidence: Evidence; detectRule: string | null; answer: Answer;
  /** How often this type has come up lately, e.g. "Suggested 3 times this month". */
  seen: string;
};

export type Verdict = "too_early" | "none" | "better" | "worse" | "unclear";
export const VERDICT_WORD: Record<Verdict, string> = { too_early: "Too early to tell", none: "No clear change", better: "Looks better · linked, not proven", worse: "Looks worse", unclear: "Can't tell" };
export const VERDICT_SHORT: Record<Verdict, string> = { too_early: "Too early", none: "No change", better: "Better", worse: "Worse", unclear: "Can't tell" };

export type Stat = { label: string; n: number; mean: string; unit: string };
export type Point = { x: number; y: number; g: "done" | "other" | "before" };
/** How one recommendation type has gone over time (Taisun: group outcomes by type). */
export type Period = { label: string; suggested: number; done: number; diff: string | null; verdict: Verdict };
export type TypeOutcome = {
  type: string; page: TakePage; title: string; metric: string; verdict: Verdict; window: string;
  suggested: number; done: number; detected: number;
  headline: string; sentence: string; stats: Stat[]; diff: string | null; range: string | null; wobble: string | null;
  checked: string; excluded: string; periods: Period[]; points: Point[];
};

export type TodayState =
  | { kind: "suggestion"; suggestion: Suggestion }
  | { kind: "learning"; have: number; need: number; unit: string }
  | { kind: "none_today"; why: string }
  | { kind: "followup"; outcome: TypeOutcome };

export type MemoryLine = { day: string; text: string };

export type HistoryFilter = "all" | "suggestion" | "followup";
export type HistoryEntry = { id: string; week: string; day: string; weekday: string; text: string; kind: "daily" | "followup"; suggestion: { title: string; answer: Answer } | null; verdict: Verdict | null };

export type WeekRow = { page: TakePage; title: string; sub: string; days: Array<AnswerState | "detected" | null>; done: number; of: number; verdict: Verdict; note: string };
export type WeekSuggestions = { done: number; of: number; detected: number; skipped: number; notToday: number; unknown: number; rows: WeekRow[]; change: { type: string; title: string; body: string } | null; takes: Array<{ text: string; from: string }> };

export type ReadyToKeep = { outcome: TypeOutcome; note: string };
export type Lately = { page: TakePage; title: string; sub: string; days: Array<AnswerState | "detected" | null> };
export type GoalsFromTakes = { ready: ReadyToKeep[]; lately: Lately[] };

export type TakeSettings = { keepDays: 365; summarizeAfter: true; autoDone: boolean };
export type Action = Wired<{ ok: true }>;

export interface TakesAdapter {
  today(page: TakePage): Wired<TodayState>;
  memory(page: TakePage): Wired<MemoryLine | null>;
  historyCount(page: TakePage): Wired<number>;
  history(page: TakePage, filter: HistoryFilter, q: string): Wired<HistoryEntry[]>;
  answer(page: TakePage, id: string, state: AnswerState, reason?: SkipReason | null): Promise<Action>;
  undo(page: TakePage, id: string): Promise<Action>;
  forget(page: TakePage, id: string): Promise<Action>;
  typeAction(type: string, action: "keep" | "keep_suggesting" | "stop"): Promise<Action>;
  outcomes(): Wired<TypeOutcome[]>;
  week(): Wired<WeekSuggestions>;
  goals(): Wired<GoalsFromTakes>;
  settings(): Wired<TakeSettings>;
  setAutoDone(on: boolean): Promise<Action>;
  /** Tap only until a detector exists for this page (HANDOFF-1111t §4). */
  detects(page: TakePage): boolean;
}

const never = async (): Promise<Action> => UNWIRED;
export const unwiredTakes: TakesAdapter = {
  today: () => UNWIRED, memory: () => UNWIRED, historyCount: () => UNWIRED, history: () => UNWIRED,
  answer: never, undo: never, forget: never, typeAction: never,
  outcomes: () => UNWIRED, week: () => UNWIRED, goals: () => UNWIRED, settings: () => UNWIRED, setAutoDone: never,
  detects: () => false,
};

/**
 * The app (HANDOFF-1111t §11 step 1): the take log is wired (`memory`, `historyCount`, `history`, `forget`,
 * `settings`); everything else stays unwired. Reads are synchronous over the cached log (takeLog.ts); a log that
 * has not loaded yet reads as unwired, never as empty.
 */
export const liveTakes: TakesAdapter = {
  ...unwiredTakes,
  memory: (page) => {
    ensureTakeLog(); const log = takeLogEntries(); if (!log) return UNWIRED;
    const now = Date.now();
    return { state: "live", value: memoryLineView(log, page, tokyoDay(new Date(now).toISOString()) ?? "", now) };
  },
  historyCount: (page) => {
    ensureTakeLog(); const log = takeLogEntries(); if (!log) return UNWIRED;
    return { state: "live", value: historyView(log, page, "all", "", Date.now()).length };
  },
  history: (page, filter, q) => {
    ensureTakeLog(); const log = takeLogEntries(); if (!log) return UNWIRED;
    return { state: "live", value: historyView(log, page, filter, q, Date.now()) };
  },
  forget: async (_page, id) => ((await forgetTake(id)) ? { state: "live", value: { ok: true } } : UNWIRED),
  // Steps 2 to 5 (HANDOFF-1111t §11): selection, answers, detectors and outcomes live in liveSuggest.ts.
  today: liveSuggestMethods.today, answer: liveSuggestMethods.answer, undo: liveSuggestMethods.undo, typeAction: liveSuggestMethods.typeAction,
  outcomes: liveSuggestMethods.outcomes, week: liveSuggestMethods.week, goals: liveSuggestMethods.goals,
  settings: liveSuggestMethods.settings, setAutoDone: liveSuggestMethods.setAutoDone, detects: liveSuggestMethods.detects,
};

// ---------------- sample (preview and tests only; numbers match the 1111t mockups) ----------------

const P = (cite: string): Source => ({ cite, verified: false });

/** Agents and Smart home have a take but no suggestion, memory or sample history of their own. */
const NO_SAMPLE: readonly TakePage[] = ["agents", "smarthome"];
type SamplePage = Exclude<TakePage, "agents" | "smarthome">;
const SUGGESTIONS: Record<SamplePage, Suggestion> = {
  neuro: {
    id: "s-neuro-1028", page: "neuro", type: "breathe_before_session", typeLabel: "Breathing before a session", seen: "First time this week",
    title: "Breathe slowly for 5 minutes right before your next Muse session.",
    how: "About 6 breaths a minute: in for 4 seconds, out for 6. The Muse breathing guide can pace it, or do it on your own.",
    why: "Calm averaged **58** over your last 7 sessions, below your usual **64**. Last night's **71** was your best in 9 days, so this isn't a slump, just a lower stretch.",
    facts: [{ label: "Calm · last 7 sessions", value: "58", note: "usual 64 · 28 days" }, { label: "Calm · last night", value: "71", note: "Oct 27, 20 min" }, { label: "Focus · last 7", value: "60", note: "usual 62" }],
    evidence: { level: "moderate", summary: "Slow breathing near 6 a minute raised heart-rate variability and lowered self-rated anxiety across small studies. Effects are short-term; long-term evidence is thinner.",
      sources: [P("Systematic review of slow breathing, 15 studies (Zaccaro 2018)"), P("HRV-biofeedback meta-analysis, 58 studies (Lehrer 2020)")],
      alsoConsidered: "a 10-minute walk outside (no clear change for you so far) · reading before a session (weak: one unpublished 2009 test)." },
    detectRule: "Counts by itself if a Muse breathing session of 4+ min ends within 30 min before a session.", answer: { state: "waiting" },
  },
  fitness: {
    id: "s-fit-1028", page: "fitness", type: "brisk_walk", typeLabel: "Brisk walk before 5 PM", seen: "Suggested on 9 days since Oct 20",
    title: "Take a 25-minute brisk walk outside before 5 PM.",
    how: "Brisk means you can talk but not sing. Outside beats a treadmill: you get the daylight too.",
    why: "You fell asleep in **24 min** on average this week, against your usual **14**. Days with an afternoon walk had shorter sleep starts last month.",
    facts: [{ label: "Falling asleep · this week", value: "24 min", note: "usual 14 · 28 days" }, { label: "Steps by 5 PM", value: "3,840", note: "usual 6,100" }, { label: "Recovery", value: "74%", note: "room for effort" }],
    evidence: { level: "strong", summary: "Regular moderate activity improves sleep quality and how fast people fall asleep in large reviews.", sources: [P("2018 US Physical Activity Guidelines Advisory Committee report")], alsoConsidered: "a late gym session (can delay sleep when it ends near bedtime)." },
    detectRule: "Counts by itself from WHOOP: a walk of 20+ min, or 2,500+ steps within 30 min, before 5 PM.",
    answer: { state: "done", source: "detected", at: "4:12 PM", detail: "WHOOP: 27-min walk, 3,110 steps, 3:45–4:12 PM" },
  },
  mindBody: {
    id: "s-mb-1028", page: "mindBody", type: "easy_day_low_hrv", typeLabel: "Easy day when HRV is low", seen: "On low-HRV days only · 3 this week",
    title: "Keep today's training easy: a walk or mobility, not intervals.",
    how: "Easy means effort under 10 on WHOOP. A hard day can wait until HRV is back near your usual.",
    why: "HRV is **56 ms**, under your usual **60**, and resting HR is steady at **50**. Calm held at **71** last night, so this is the body asking, not the mind.",
    facts: [{ label: "HRV · this morning", value: "56 ms", note: "usual 60 · 28 days" }, { label: "Resting HR", value: "50 bpm", note: "usual 52" }, { label: "Calm · last night", value: "71", note: "Oct 27" }],
    evidence: { level: "moderate", summary: "Adjusting training to morning HRV gave similar or better fitness with fewer hard days in small trials.", sources: [P("HRV-guided training trial (Kiviniemi 2007)")] },
    detectRule: "Counts by itself if WHOOP effort stays under 10 by midnight.", answer: { state: "waiting" },
  },
  business: {
    id: "s-biz-1028", page: "business", type: "decide_before_noon", typeLabel: "Top decision before noon", seen: "Suggested on 8 days since Oct 12",
    title: "Decide the Taiyo Skin funding vote before noon.",
    how: "Read the two options in Enterprise › Decisions, pick one, and record it. Amara won't vote for you.",
    why: "The vote has waited **5 days**. Your decisions made before noon closed **1.8 days** sooner last month, from your own ledger.",
    facts: [{ label: "Waiting", value: "5 days", note: "opened Oct 23" }, { label: "Decided before noon", value: "1.4 days", note: "7 decisions" }, { label: "Decided later", value: "3.2 days", note: "6 decisions" }],
    evidence: { level: "own", summary: "From your own decisions only. There's no outside science claim for business habits." , sources: [] },
    detectRule: "Counts by itself when the decision is recorded in the decisions log before 12:00 JST.", answer: { state: "waiting" },
  },
  home: {
    id: "s-home-1028", page: "home", type: "warm_lights", typeLabel: "Lights warm after 9 PM", seen: "Suggested on 9 evenings since Oct 12",
    title: "Lights warm and dim after 9 PM.",
    how: "Switch the living room and bedroom to the warm evening scene yourself. Amara only suggests and checks.",
    why: "You fell asleep in **24 min** this week against your usual **14**.",
    facts: [{ label: "Falling asleep", value: "24 min", note: "usual 14" }],
    evidence: { level: "moderate", summary: "Bright room light in the evening delayed melatonin in a lab study; dimmer, warmer light did less.", sources: [P("Room light and melatonin (Gooley 2011)")] },
    detectRule: "Counts by itself if Home Assistant shows the evening warm scene on at 9 PM ± 15 min.", answer: { state: "waiting" },
  },
  life: {
    id: "s-life-1111", page: "life", type: "read_before_bed", typeLabel: "Read before bed", seen: "First time this week",
    title: "Keep tonight's read-before-bed habit.",
    how: "Twenty minutes before bed, as you wrote it. Five nights a week is the target.",
    why: "You've kept it **4 of 5** nights this week. The lease notice, due **Mar 1**, is the other Life item that's coming up.",
    facts: [{ label: "This week's habit", value: "4 of 5", note: "read before bed" }, { label: "Lease notice", value: "Mar 1", note: "Dates & deadlines" }],
    evidence: { level: "moderate", summary: "A short, regular wind-down before bed is a habit you set. Amara doesn't treat this as medical advice.", sources: [] },
    detectRule: "Counts when you mark the habit done.",
    answer: { state: "waiting" },
  },
};

const BREATH_POINTS: Point[] = [
  ...[57, 60, 55, 59, 58, 54, 61, 56, 58, 57, 59, 60].map((y, i) => ({ x: i, y, g: "before" as const })),
  ...[67, 64, 70, 63, 69, 72, 66, 68, 71, 65, 69].map((y, i) => ({ x: 13 + i * 2, y, g: "done" as const })),
  ...[61, 58, 63, 59, 60, 59].map((y, i) => ({ x: 14 + i * 3, y, g: "other" as const })),
];

const OUTCOMES: TypeOutcome[] = [
  { type: "breathe_before_session", page: "neuro", title: "Breathing before a session", metric: "calm in that session", verdict: "better", window: "Oct 28 – Nov 18",
    suggested: 17, done: 11, detected: 6,
    headline: "Calm is about **7 points higher** on days you breathed first.",
    sentence: "Likely range **+2 to +12**. That's bigger than your normal day-to-day wobble (±3), so it's probably not noise. It doesn't show that the breathing did it.",
    stats: [{ label: "Breathed first", n: 11, mean: "67", unit: "sessions" }, { label: "Suggested, not done", n: 6, mean: "60", unit: "sessions" }, { label: "Before it was suggested", n: 12, mean: "58", unit: "sessions" }],
    diff: "+7", range: "+2 to +12", wobble: "±3",
    checked: "Sleep was similar on both sets of days (7h 21m vs 7h 14m), and so was recovery (68% vs 66%).", excluded: "Unknown days (1) and \"not today\" days (0) are left out.",
    periods: [{ label: "Oct 28 – Nov 3", suggested: 6, done: 4, diff: null, verdict: "too_early" }, { label: "Nov 4 – Nov 10", suggested: 5, done: 3, diff: "+6", verdict: "too_early" }, { label: "Nov 11 – Nov 18", suggested: 6, done: 4, diff: "+7", verdict: "better" }],
    points: BREATH_POINTS },
  { type: "warm_lights", page: "home", title: "Lights warm after 9 PM", metric: "time to fall asleep", verdict: "better", window: "Oct 12 – Oct 25",
    suggested: 17, done: 9, detected: 6,
    headline: "You fell asleep about **19 min sooner** on warm-light nights.",
    sentence: "Likely range **4 to 33** min (9 nights vs 8). Your sleep schedule was the same on both, so it's a fair comparison, though not proof.",
    stats: [{ label: "Warm lights", n: 9, mean: "13 min", unit: "nights" }, { label: "Suggested, not done", n: 8, mean: "32 min", unit: "nights" }],
    diff: "−19 min", range: "4 to 33", wobble: "±6 min", checked: "Bedtime was similar (23:41 vs 23:38).", excluded: "Unknown nights (2) are left out.",
    periods: [{ label: "Oct 12 – Oct 18", suggested: 8, done: 4, diff: null, verdict: "too_early" }, { label: "Oct 19 – Oct 25", suggested: 9, done: 5, diff: "−19 min", verdict: "better" }], points: [] },
  { type: "easy_day_low_hrv", page: "mindBody", title: "Easy day when HRV is low", metric: "next-morning HRV", verdict: "better", window: "Sep 28 – Oct 26",
    suggested: 13, done: 7, detected: 7, headline: "Next-morning HRV was about **4 ms higher** after easy days.", sentence: "Likely range **+1 to +7** ms (7 vs 6 mornings).",
    stats: [{ label: "Kept it easy", n: 7, mean: "61 ms", unit: "mornings" }, { label: "Suggested, not done", n: 6, mean: "57 ms", unit: "mornings" }],
    diff: "+4 ms", range: "+1 to +7", wobble: "±3 ms", checked: "Sleep was similar (7h 20m vs 7h 12m).", excluded: "No days left out.",
    periods: [{ label: "Sep 28 – Oct 11", suggested: 6, done: 3, diff: null, verdict: "too_early" }, { label: "Oct 12 – Oct 26", suggested: 7, done: 4, diff: "+4 ms", verdict: "better" }], points: [] },
  { type: "walk_before_session", page: "neuro", title: "Walk 10 min before a session", metric: "calm in that session", verdict: "none", window: "Oct 7 – Oct 27",
    suggested: 19, done: 12, detected: 3, headline: "Calm was **+1** on walk days.", sentence: "The range is **−4 to +6**, so it includes zero and sits inside your normal wobble (±4). No clear change. Amara stops suggesting it for Neuro for 60 days.",
    stats: [{ label: "Walked first", n: 12, mean: "60", unit: "sessions" }, { label: "Suggested, not done", n: 7, mean: "59", unit: "sessions" }, { label: "Before it was suggested", n: 12, mean: "59", unit: "sessions" }],
    diff: "+1", range: "−4 to +6", wobble: "±4", checked: "Sleep and recovery were similar (7h 16m vs 7h 09m; 67% vs 65%).", excluded: "Unknown days (2) and \"not today\" days (1) are left out.",
    periods: [{ label: "Oct 7 – Oct 13", suggested: 7, done: 4, diff: null, verdict: "too_early" }, { label: "Oct 14 – Oct 20", suggested: 6, done: 4, diff: "+2", verdict: "too_early" }, { label: "Oct 21 – Oct 27", suggested: 6, done: 4, diff: "+1", verdict: "none" }], points: [] },
  { type: "decide_before_noon", page: "business", title: "Top decision before noon", metric: "days to decide", verdict: "unclear", window: "Oct 12 – Oct 26",
    suggested: 13, done: 7, detected: 7, headline: "Decisions closed **1.8 days** sooner.", sentence: "But **5 of the 7** done days were in launch week, when everything moved faster. Too much else changed to credit the timing.",
    stats: [{ label: "Decided before noon", n: 7, mean: "1.4 days", unit: "decisions" }, { label: "Decided later", n: 6, mean: "3.2 days", unit: "decisions" }],
    diff: "−1.8 days", range: null, wobble: null, checked: "Launch-week share: 5 of 7 vs 1 of 6, so the comparison is confounded.", excluded: "No days left out.",
    periods: [{ label: "Oct 12 – Oct 19", suggested: 7, done: 3, diff: null, verdict: "too_early" }, { label: "Oct 20 – Oct 26", suggested: 6, done: 4, diff: "−1.8 days", verdict: "unclear" }], points: [] },
  { type: "hard_on_green", page: "fitness", title: "Harder session on green-recovery days", metric: "next-morning HRV", verdict: "worse", window: "Sep 27 – Oct 18",
    suggested: 19, done: 8, detected: 8, headline: "Next-morning HRV was **5 ms lower** after those days.", sentence: "Range **−9 to −1**, and recovery 6 points lower. Amara has stopped suggesting it. If you want to keep training hard, pair it with an easy day after.",
    stats: [{ label: "Trained hard", n: 8, mean: "54 ms", unit: "mornings" }, { label: "Suggested, not done", n: 11, mean: "59 ms", unit: "mornings" }],
    diff: "−5 ms", range: "−9 to −1", wobble: "±3 ms", checked: "Sleep was similar (7h 05m vs 7h 11m).", excluded: "Unknown days (1) are left out.",
    periods: [{ label: "Sep 27 – Oct 10", suggested: 10, done: 4, diff: null, verdict: "too_early" }, { label: "Oct 11 – Oct 18", suggested: 9, done: 4, diff: "−5 ms", verdict: "worse" }], points: [] },
  { type: "brisk_walk", page: "fitness", title: "Brisk walk before 5 PM", metric: "time to fall asleep", verdict: "too_early", window: "Oct 20 – today",
    suggested: 9, done: 7, detected: 5, headline: "**7** walks so far, **2** days without.", sentence: "Amara needs at least **7 + 7** days before she says anything. First look: **Nov 3**.",
    stats: [{ label: "Walked", n: 7, mean: "17 min", unit: "nights" }, { label: "Suggested, not done", n: 2, mean: "22 min", unit: "nights" }],
    diff: null, range: null, wobble: null, checked: "Not compared yet.", excluded: "No days left out.",
    periods: [{ label: "Oct 20 – today", suggested: 9, done: 7, diff: null, verdict: "too_early" }], points: [] },
];

const MEMORY: Record<SamplePage, MemoryLine | null> = {
  neuro: { day: "Oct 27", text: "**Oct 7–27** I suggested a 10-minute walk before sessions on **19** days. You did it on **12**; calm was **+1** on walk days (range −4 to +6), so no clear change. Today I'm suggesting something different." },
  fitness: { day: "Oct 20", text: "Since **Oct 20** I've suggested a brisk walk on **9** days and you did it on **7**. I won't say whether it helps your sleep until there are 7 days each way: first look **Nov 3**." },
  mindBody: { day: "Oct 26", text: "Easy days when HRV is low: next-morning HRV **+4 ms** (range +1 to +7) over **13** low-HRV days. It's now one of your habits." },
  business: { day: "Oct 26", text: "Deciding before noon: **1.8 days** faster, but launch week skewed it, so I can't tell yet." },
  home: null,
  life: null,
};

const HISTORY: HistoryEntry[] = [
  { id: "h1", week: "This week", day: "Oct 28", weekday: "Wed", kind: "daily", text: "Calm sits at **58** over 7 sessions, under your usual **64**. Suggesting slow breathing before a session.", suggestion: { title: "Breathe 5 min before a session", answer: { state: "waiting" } }, verdict: null },
  { id: "h2", week: "This week", day: "Oct 27", weekday: "Tue", kind: "followup", text: "Walks before sessions, Oct 7–27: calm **+1** on walk days (range −4 to +6). **No clear change.**", suggestion: { title: "Walk 10 min before a session", answer: { state: "done", source: "detected" } }, verdict: "none" },
  { id: "h3", week: "This week", day: "Oct 26", weekday: "Mon", kind: "daily", text: "Focus **66** after **8h 02m** of sleep, your best this month.", suggestion: { title: "Walk 10 min before a session", answer: { state: "done", source: "tap" } }, verdict: null },
  { id: "h4", week: "This week", day: "Oct 25", weekday: "Sun", kind: "daily", text: "No session yesterday. Calm last ran **55** on Friday.", suggestion: { title: "Walk 10 min before a session", answer: { state: "unknown" } }, verdict: null },
  { id: "h5", week: "Last week", day: "Oct 21", weekday: "Wed", kind: "followup", text: "Walks before sessions: **7** days done. Too early to tell; calm **60 vs 58**.", suggestion: { title: "Walk 10 min before a session", answer: { state: "done", source: "tap" } }, verdict: "too_early" },
  { id: "h6", week: "Last week", day: "Oct 20", weekday: "Tue", kind: "daily", text: "Calm **54**, lowest in 3 weeks, after a **5h 50m** night.", suggestion: { title: "Walk 10 min before a session", answer: { state: "skipped", source: "tap", reason: "no_time" } }, verdict: null },
];

const WEEK: WeekSuggestions = {
  done: 22, of: 30, detected: 12, skipped: 5, notToday: 1, unknown: 2,
  rows: [
    { page: "neuro", title: "Walk 10 min before a session", sub: "Suggested 6 of 7 days", days: ["done", "detected", "done", "skipped", "done", "detected", null], done: 4, of: 6, verdict: "too_early", note: "calm 60 vs 58" },
    { page: "fitness", title: "25-min brisk walk outside", sub: "Suggested 6 of 7 days · started Oct 20", days: [null, "detected", "detected", "done", "detected", "detected", "skipped"], done: 5, of: 6, verdict: "too_early", note: "first look Nov 3" },
    { page: "mindBody", title: "Easy training when HRV is under your usual", sub: "Only on low-HRV days · 3 this week", days: [null, "detected", null, null, "skipped", "detected", null], done: 2, of: 3, verdict: "better", note: "next-day HRV +4 ms (+1 to +7)" },
    { page: "business", title: "Top decision before noon", sub: "Suggested on workdays", days: ["detected", "done", "skipped", "detected", "detected", null, null], done: 4, of: 5, verdict: "unclear", note: "launch week changed too much" },
    { page: "home", title: "Lights warm and dim after 9 PM", sub: "You switch the scene; Amara checks it", days: ["detected", "detected", "done", "detected", "unknown", "detected", "done"], done: 6, of: 7, verdict: "better", note: "asleep 19 min sooner (4 to 33)" },
  ],
  change: { type: "warm_lights", title: "Make the evening lights routine a habit.", body: "It's the suggestion with the clearest result: asleep **19 min** sooner on warm-light nights (range **4 to 33**, 9 nights vs 8). Your sleep schedule was the same on both, so it's a fair comparison, though not proof." },
  takes: [{ text: "Walks before sessions: too early to tell.", from: "Neuro, Wed" }, { text: "HRV 52, under your 60. Keep today easy.", from: "Mind–Body, Thu" }, { text: "Two decisions waiting 5 days.", from: "Business, Fri" }],
};

const GOALS: GoalsFromTakes = {
  ready: [{ outcome: OUTCOMES[1], note: "You switched the scene yourself on 9 of 17 suggested nights." }],
  lately: [
    { page: "neuro", title: "Neuro · breathe before a session", sub: "New today", days: [null, null, null, null, null, null, "waiting"] },
    { page: "fitness", title: "Fitness · 25-min brisk walk", sub: "9 days suggested · first look Nov 3", days: ["detected", "detected", "done", "detected", "skipped", "detected", "detected"] },
  ],
};

export type TakesPreview = { today?: Partial<Record<TakePage, "suggestion" | "learning" | "none_today" | "followup">>; answer?: Partial<Record<TakePage, AnswerState | "detected">>; unwired?: Array<"suggestions" | "detection" | "history" | "outcomes">; historyEmpty?: boolean };

export function sampleTakes(opts: TakesPreview = {}): TakesAdapter {
  const answers = new Map<string, Answer>();
  const forgotten = new Set<string>();
  let autoDone = true;
  const off = new Set(opts.unwired ?? []);
  const suggestionFor = (page: TakePage): Suggestion => {
    const base = SUGGESTIONS[page as SamplePage];
    const forced = opts.answer?.[page];
    const preset: Answer | null = !forced ? null : forced === "detected" ? (SUGGESTIONS.fitness.answer) : forced === "done" ? { state: "done", source: "tap", at: "8:12 PM" } : forced === "skipped" ? { state: "skipped", source: "tap", at: "8:40 PM", reason: null } : { state: forced };
    const answer = answers.get(base.id) ?? preset ?? base.answer;
    return { ...base, answer: forced === "detected" ? { ...SUGGESTIONS.fitness.answer, detail: page === "neuro" ? "Muse: 5-min breathing, 8:04 PM, then a session at 8:10" : SUGGESTIONS.fitness.answer.detail } : answer, detectRule: off.has("detection") ? null : base.detectRule };
  };
  return {
    today: (page) => {
      if (off.has("suggestions") || NO_SAMPLE.includes(page)) return UNWIRED;
      const k = opts.today?.[page] ?? "suggestion";
      if (k === "learning") return sample({ kind: "learning", have: 3, need: 7, unit: "sessions" });
      if (k === "none_today") return sample({ kind: "none_today", why: "Your numbers are where they usually are. A suggestion now would be noise." });
      if (k === "followup") return sample({ kind: "followup", outcome: OUTCOMES.find((o) => o.page === page) ?? OUTCOMES[0] });
      return sample({ kind: "suggestion", suggestion: suggestionFor(page) });
    },
    memory: (page) => (off.has("suggestions") || NO_SAMPLE.includes(page) ? UNWIRED : sample(opts.today?.[page] === "learning" ? null : opts.today?.[page] === "followup" ? { day: "Nov 4", text: "On **Nov 4** I said it was too early to tell (4 done days). With **11** now, I can say more, but carefully." } : MEMORY[page as SamplePage])),
    historyCount: (page) => (off.has("history") || NO_SAMPLE.includes(page) ? UNWIRED : sample(opts.historyEmpty ? 0 : opts.today?.[page] === "learning" ? 3 : 22)),
    history: (_page, filter, q) => {
      if (off.has("history")) return UNWIRED;
      if (opts.historyEmpty) return sample([]);
      const ql = q.trim().toLowerCase();
      return sample(HISTORY.filter((h) => !forgotten.has(h.id))
        .filter((h) => filter === "all" || (filter === "suggestion" ? h.suggestion !== null : h.kind === "followup"))
        .filter((h) => !ql || `${h.text} ${h.suggestion?.title ?? ""}`.toLowerCase().includes(ql)));
    },
    answer: async (page, id, state, reason = null) => { answers.set(id, { state, source: "tap", at: "now", reason }); void page; return sample({ ok: true }); },
    undo: async (_page, id) => { answers.set(id, { state: "waiting" }); return sample({ ok: true }); },
    forget: async (_page, id) => { forgotten.add(id); return sample({ ok: true }); },
    typeAction: async () => sample({ ok: true }),
    outcomes: () => (off.has("outcomes") ? UNWIRED : sample(OUTCOMES)),
    week: () => (off.has("outcomes") ? UNWIRED : sample(WEEK)),
    goals: () => (off.has("outcomes") ? UNWIRED : sample(GOALS)),
    settings: () => sample({ keepDays: 365, summarizeAfter: true, autoDone }),
    setAutoDone: async (on) => { autoDone = on; return sample({ ok: true }); },
    detects: () => !off.has("detection"),
  };
}

/** Preview flags (localStorage "nexus.takesPreview", JSON), read only in sample mode. Never affects the live app. */
export const TAKES_PREVIEW_KEY = "nexus.takesPreview";
function previewOpts(): TakesPreview {
  try { const raw = globalThis.localStorage?.getItem(TAKES_PREVIEW_KEY); return raw ? JSON.parse(raw) as TakesPreview : {}; } catch { return {}; }
}
let cached: TakesAdapter | null = null;
export function takesAdapter(): TakesAdapter {
  if (!sampleAdaptersEnabled()) return liveTakes;
  return (cached ??= sampleTakes(previewOpts()));
}
export function resetTakesAdapter() { cached = null; }

export { CAUSAL } from "./causal";
