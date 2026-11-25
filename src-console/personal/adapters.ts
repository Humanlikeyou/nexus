import { personalMoneyFrom } from "../money/personalMoney";
import { mapFx } from "../money/live";
import type { RawSummary } from "../money/overview";
import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import { patternsFrom, weeklyReviewFrom, type PatternsWire, type ReviewWire, type ExperimentResultWire } from "./liveInsights";
import { dictateOnce } from "../adds/dictation";
import { roadmapAdapter } from "../goals/roadmap";
import { listen } from "@tauri-apps/api/event";
import { ADDS_EVENT, announceAdds, announceSnapshot, announceStores, inTauri, live, realInvoke, type Invoke } from "../lib/localStores";

/**
 * design-1111a: Amara as a personal OS. Every surface reads from this adapter. In the app it is
 * unwired ("Needs wiring") until Claude builds the stores and commands in HANDOFF-1111a.md; the
 * sample adapter is preview/test only and every value it returns carries a "Sample data" tag.
 * Real visa dates, money accounts and the JPY→USD rule come from Taisun later: never invented here.
 */

// ---- Goals and habits ----
export type DayMark = "met" | "nearly" | "missed" | "none";
export type Habit = { id: string; title: string; source: string; rule: string; days: DayMark[]; done: number; target: number; note: string;
  /** Why it matters, `HH:MM` JST reminder and pause, from the goals row (decision 0045). */
  why?: string; remindAt?: string; paused?: boolean;
  /** Today's mark from the habit-day store: done, marked not done, or not marked. Absent when no store is wired. */
  today?: "done" | "not" | "open" };
export type Milestone = { id: string; title: string; detail: string; right: string; pct: number; tone?: "copper" };
export type GoalsWeek = { habits: Habit[]; milestones: Milestone[]; nextMilestone: string | null };

// ---- Health patterns ----
export type Finding = {
  id: string; title: string; confidence: 1 | 2 | 3 | 4 | 5; strength: string; n: string; sentence: string; detail: string;
  chart: { kind: "scatter"; points: Array<[number, number]>; split: number; x: [number, number]; y: [number, number]; xLabel: string; yLabel: string } | { kind: "bars"; rows: Array<[string, number, number]>; legend: [string, string] };
  chips?: string[];
};
export type Patterns = { headline: string; window: string; findings: Finding[] };

// ---- Check-ins ----
/** 5-step mood (Taisun, 12:52 JST Nov 11). */
export const MOOD_STEPS = ["Very unpleasant", "Unpleasant", "Neutral", "Pleasant", "Very pleasant"] as const;
export const ENERGY_STEPS = ["Drained", "Low", "OK", "Good", "Charged"] as const;
export type CheckIn = { id: string; when: string; mood: number; energy: number; tags: string[]; text: string; transcribed?: boolean };
export type CheckInDraft = { mood: number | null; energy: number | null; tags: string[]; text: string };

// ---- Personal money (USD, never mixed with Business) ----
export type Bill = { id: string; name: string; sub: string; due: string; amount: Wired<number>; status: "scheduled" | "pay" | "auto" };
export type Subscription = { id: string; name: string; sub: string; amount: Wired<number>; unusedDays?: number };
export type PersonalMoney = {
  month: string; leftAfterBills: Wired<number>; spent: Wired<number>; spentNote: string; subsMonthly: Wired<number>; subsNote: string;
  billsDue: Wired<number>; billsNote: string; payday: string; bills: Bill[]; subscriptions: Subscription[]; moreSubs: string;
  categories: Array<{ name: string; amount: number; pct: number; up?: boolean }>; summary: string;
};

// ---- Memory ----
export type MemoryCategory = "about" | "health" | "work" | "preferences" | "people";
export const MEMORY_LABEL: Record<MemoryCategory, string> = { about: "About you", health: "Health", work: "Work", preferences: "Preferences", people: "People" };
export type Memory = { id: string; text: string; category: MemoryCategory; source: string };
export type PendingMemory = { id: string; text: string; source: string; category: MemoryCategory };
export type MemoryWrite = Wired<{ ok: true }>;
/** Row as src-tauri memory_read.rs `memory_list` returns it (read-only Letta core-memory blocks). */
export type MemoryWire = { id: string; label: string; value: string; agent: string; updatedAt?: string | null; truncated?: boolean };

// ---- Life admin ----
/** design-1111v: three lanes. "enterprise" dates are read-only in Life (they live in Enterprise › Ventures).
 *  `month` is months from Nov 1 on the 12-month timeline; `short` is its timeline label. */
export type LifeLane = "personal" | "residence" | "enterprise";
export type LifeDate = { id: string; title: string; short?: string; sub: string; date: string; month: number; inDays?: number; reminder: string; due: boolean; lane: LifeLane;
  /** The stored row (YYYY-MM-DD and the typed sub line), for an inline edit or an Undo that puts it back. */
  iso?: string; rawSub?: string;
  /** 0045: own reminder offsets (days before, largest first; empty = 90, 30, 7), done, and the goal it belongs to. */
  leadDays?: number[]; done?: boolean; goalId?: string | null };
export type LifeAdmin = { next: string; dates: LifeDate[] };

// ---- Weekly review ----
export type WeeklyReview = {
  title: string; week: string;
  tiles: Array<{ key: string; big: string; unit: string; delta: string; up: boolean; sub: string }>;
  /** `findingId` is the published finding behind the suggestion (Mind–Body › Patterns); both are absent when there is no suggestion. */
  change: { title: string; body: string; findingId?: string };
  /** `goalId` is set on a goal's row so it can open that goal in focus. */
  highlights: Array<{ ok: boolean; title: string; sub: string; goalId?: string }>;
  notes: Array<{ text: string; day: string }>;
  /** "Try it this week" results due this week (weekly_review.rs `experiments`), passed through as the backend sent them; the Weekly review shows them as "Last week's try". */
  experiments?: ExperimentResultWire[];
};

export interface PersonalAdapter {
  goals(): Wired<GoalsWeek>;
  patterns(): Wired<Patterns>;
  checkins(): Wired<CheckIn[]>;
  saveCheckIn(d: CheckInDraft): Promise<Wired<CheckIn>>;
  /** Local speech-to-text: the transcript fills the text box; no audio is stored. Unwired until a transcription path exists. */
  transcribe(): Promise<Wired<string>>;
  money(): Wired<PersonalMoney>;
  memories(): Wired<Memory[]>;
  /** Why `memories()` is unwired when a live read failed (plain words, fixed per error code); null otherwise. */
  memoriesNote?(): string | null;
  pendingMemories(): Wired<PendingMemory[]>;
  editMemory(id: string, text: string): Promise<MemoryWrite>;
  deleteMemory(id: string): Promise<MemoryWrite>;
  decideMemory(id: string, keep: boolean): Promise<MemoryWrite>;
  /** Undo: puts back the text the last edit or delete replaced (Letta rows only). The Memory page offers it for 8 s. */
  restoreMemory?(id: string): Promise<MemoryWrite>;
  /** Plain-words result of the last edit, delete or undo (also set on success); null before any. */
  memoryWriteNote?(): string | null;
  lifeAdmin(): Wired<LifeAdmin>;
  weeklyReview(): Wired<WeeklyReview>;
  /** "Try it this week" sets a reminder; it never changes a goal by itself. */
  tryChange(take: boolean): Promise<Wired<{ reminder: string | null }>>;
}

export const unwiredPersonal: PersonalAdapter = {
  goals: () => UNWIRED, patterns: () => UNWIRED, checkins: () => UNWIRED, saveCheckIn: async () => UNWIRED, transcribe: async () => UNWIRED,
  money: () => UNWIRED, memories: () => UNWIRED, pendingMemories: () => UNWIRED, editMemory: async () => UNWIRED, deleteMemory: async () => UNWIRED,
  decideMemory: async () => UNWIRED, lifeAdmin: () => UNWIRED, weeklyReview: () => UNWIRED, tryChange: async () => UNWIRED,
};

const d = (s: string): DayMark[] => s.split("").map((c) => (c === "1" ? "met" : c === "h" ? "nearly" : c === "0" ? "missed" : "none"));
const usd = (v: number) => sample(v);

/** Preview/test only: the approved mockups' values (design-1111a), with the mood scale moved to 5 steps. */
export function makeSamplePersonal(): PersonalAdapter {
  let memories: Memory[] = [
    { id: "m1", text: "Lives in Tokyo; time zone JST", category: "about", source: "You told Amara · Oct 2" },
    { id: "m2", text: "Prefers short, direct answers with numbers", category: "about", source: "Learned from chats · Oct 9" },
    { id: "m3", text: "Usually trains in the evening, 18:00–19:30", category: "about", source: "From Fitness · Oct 14" },
    { id: "m4", text: "Calls the business ventures \u201cthe ventures\u201d", category: "about", source: "Learned from chats · Oct 20" },
    { id: "m5", text: "Usual HRV is about 60 ms", category: "health", source: "From Fitness · updated weekly" },
    { id: "m6", text: "Focus is higher after 7 h+ sleep", category: "health", source: "From Patterns · Oct 25" },
    { id: "m7", text: "Muse 5th electrode sits at the back of the head", category: "health", source: "You told Amara · Nov 10" },
  ];
  let pending: PendingMemory[] = [
    { id: "p1", text: "Late coffee seems to cut your afternoon energy.", source: "From your Oct 28 check-in and 3 earlier ones · Health", category: "health" },
    { id: "p2", text: "Taiyo Skin launch date is Nov 21.", source: "From Business › Ventures, Oct 27 · Work", category: "work" },
  ];
  const checkins: CheckIn[] = [
    { id: "c1", when: "Tue 10:12 PM", mood: 4, energy: 4, tags: ["deep work", "training"], text: "Good Muse session in the morning, calm the whole afternoon." },
    { id: "c2", when: "Mon 11:40 PM", mood: 3, energy: 2, tags: ["Taiyo Skin"], text: "Long supplier call, didn't get to the launch list.", transcribed: true },
  ];
  const ok = sample({ ok: true as const });
  return {
    goals: () => sample({
      nextMilestone: "Taiyo Skin launch in 24 days",
      habits: [
        { id: "h1", title: "Focus sessions", source: "Neuro", rule: "20 min with focus ≥ 60 · 5 a week", days: d("1101101"), done: 5, target: 5, note: "streak 3 wks" },
        { id: "h2", title: "Workouts", source: "Fitness", rule: "effort ≥ 40 · 4 a week", days: d("0100100"), done: 2, target: 4, note: "2 to go" },
        { id: "h3", title: "Asleep by 23:30", source: "Fitness", rule: "sleep start · 6 nights a week", days: d("1111h11"), done: 6, target: 6, note: "streak 9 nights" },
        { id: "h4", title: "7 h+ sleep", source: "Fitness", rule: "time asleep · 5 nights a week", days: d("1011011"), done: 5, target: 5, note: "on track" },
        { id: "h5", title: "Evening check-in", source: "Journal", rule: "mood + energy · daily", days: d("1110111"), done: 6, target: 7, note: "1 missed" },
      ],
      milestones: [
        { id: "ms1", title: "Revenue $5,000 / month", detail: "Business · 30-day revenue · Taiyo Skin + consulting", right: "$1,820 · 36%", pct: 36, tone: "copper" },
        { id: "ms2", title: "Taiyo Skin launch", detail: "Business › Ventures · 17 of 25 launch tasks done", right: "Nov 21 · 24 days", pct: 68 },
        { id: "ms3", title: "Amara OS v1 on this Mac", detail: "Agents › Crews · 8 of 10 stages", right: "Dec 15", pct: 80 },
      ],
    }),
    patterns: () => sample({
      headline: "Sleep is your biggest lever on focus", window: "Last 4 weeks · 14 Neuro sessions · 28 nights · updated Sunday",
      findings: [
        { id: "f1", title: "Sleep → next Neuro session", confidence: 4, strength: "Strong", n: "14 sessions", sentence: "After 7 h+ of sleep, your next session's focus averages **64**, vs **55** after shorter nights.", detail: "That's **+9 points** (8 sessions vs 6). The link is strong for you (**r = 0.90**), but it's a pattern, not proof: a short night may also mean a busier day.",
          chart: { kind: "scatter", points: [[5.6, 51], [6.1, 54], [6.3, 58], [6.6, 55], [6.8, 57], [7.0, 63], [7.2, 61], [7.4, 66], [7.5, 64], [7.7, 62], [7.9, 69], [8.1, 65], [7.3, 60], [6.4, 52]], split: 7, x: [5.5, 8.5], y: [50, 70], xLabel: "Sleep (h)", yLabel: "Next-session focus" }, chips: ["Sleep 7h 32m last night", "Focus 62 · Oct 27"] },
        { id: "f2", title: "Training load → focus", confidence: 3, strength: "Medium", n: "21 days", sentence: "The day after a hard workout (effort ≥ 70), focus averages **57**, vs **63** on other days.", detail: "**−6 points** over 5 hard days vs 16 others. Moderate workouts (40–69) show no dip.", chart: { kind: "bars", rows: [["Focus", 57, 63], ["Calm", 69, 71]], legend: ["day after hard workout", "other days"] } },
        { id: "f3", title: "HRV → calm", confidence: 2, strength: "Weak", n: "14 sessions", sentence: "Higher morning HRV goes with calmer sessions, slightly.", detail: "Above your usual **60 ms**: calm **73**. Below: calm **69**. Only **+4**, **r = 0.31**. Amara will keep watching before calling it.", chart: { kind: "bars", rows: [["Calm", 69, 73]], legend: ["HRV below 60", "HRV 60+"] } },
      ],
    }),
    checkins: () => sample([...checkins]),
    saveCheckIn: async (draft) => {
      const c: CheckIn = { id: `c${checkins.length + 1}`, when: "Just now", mood: draft.mood ?? 3, energy: draft.energy ?? 3, tags: draft.tags, text: draft.text };
      checkins.unshift(c);
      return sample(c);
    },
    transcribe: async () => UNWIRED,
    money: () => sample({
      month: "October", leftAfterBills: usd(1412), spent: usd(2186), spentNote: "$214 under your usual $2,400", subsMonthly: usd(143), subsNote: "9 active · 2 unused for ~2 months",
      billsDue: usd(1265), billsNote: "4 bills · next Oct 31", payday: "until Nov 25 payday",
      bills: [
        { id: "b1", name: "Rent", sub: "Tokyo apartment · auto-pay", due: "Oct 31 · Sat", amount: usd(980), status: "scheduled" },
        { id: "b2", name: "Credit card", sub: "statement balance", due: "Nov 2 · Mon", amount: usd(214.6), status: "pay" },
        { id: "b3", name: "Phone", sub: "mobile plan", due: "Nov 5 · Thu", amount: usd(38), status: "auto" },
        { id: "b4", name: "Health insurance", sub: "national, monthly", due: "Nov 10 · Tue", amount: usd(32.4), status: "auto" },
      ],
      subscriptions: [
        { id: "s1", name: "Claude Pro", sub: "monthly · Nov 3", amount: usd(20) },
        { id: "s2", name: "Apple One", sub: "monthly · Nov 8", amount: usd(19.95) },
        { id: "s3", name: "Gym", sub: "monthly · Nov 1 · last visit Aug 30", amount: usd(45), unusedDays: 59 },
        { id: "s4", name: "Streaming", sub: "monthly · Nov 12 · last watched Aug 12", amount: usd(15.49), unusedDays: 77 },
      ],
      moreSubs: "+ 5 more · $42.56",
      categories: [{ name: "Food", amount: 612, pct: 62 }, { name: "Transport", amount: 204, pct: 21 }, { name: "Health", amount: 148, pct: 15 }, { name: "Shopping", amount: 326, pct: 33, up: true }, { name: "Everything else", amount: 896, pct: 41 }],
      summary: "Food is **$612**, about your usual. Shopping is **$326**, up **$140** on September. Two subscriptions haven't been used in 2 months: **$60.49** a month.",
    }),
    memories: () => sample([...memories]),
    pendingMemories: () => sample([...pending]),
    editMemory: async (id, text) => { memories = memories.map((m) => (m.id === id ? { ...m, text } : m)); return ok; },
    deleteMemory: async (id) => { memories = memories.filter((m) => m.id !== id); return ok; },
    decideMemory: async (id, keep) => {
      const p = pending.find((x) => x.id === id);
      pending = pending.filter((x) => x.id !== id);
      if (keep && p) memories = [...memories, { id: `k${id}`, text: p.text, category: p.category, source: "You kept it · just now" }];
      return ok;
    },
    lifeAdmin: () => sample({
      next: "National health insurance payment in 3 days",
      // design-1111v (Taisun, 19:23 JST): Akiya is a personal item now, so its offer deadline sits in the Personal lane.
      dates: [
        { id: "l0", title: "National health insurance payment", short: "Health insurance", sub: "monthly · bank transfer", date: "Oct 31", month: 0.1, inDays: 3, reminder: "7 days · Oct 24", due: true, lane: "personal" },
        { id: "l1", title: "Taiyo Skin launch", short: "Launch", sub: "Enterprise › Ventures › Taiyo Skin", date: "Nov 21", month: 0.67, inDays: 24, reminder: "7 days · Nov 14", due: false, lane: "enterprise" },
        { id: "l2", title: "Akiya offer deadline", short: "Akiya offer", sub: "Akiya · Kamakura listing · offer + deposit", date: "Dec 18", month: 1.57, inDays: 51, reminder: "30 days · Nov 18", due: false, lane: "personal" },
        { id: "l3", title: "Taiyo Skin tax filing", short: "Tax filing", sub: "Enterprise › Finances · with the accountant", date: "Feb 15", month: 3.47, reminder: "90 days · Nov 17", due: false, lane: "enterprise" },
        { id: "l4", title: "Apartment contract renewal", short: "Lease renewal", sub: "2-year lease · notice 1 month before", date: "Apr 1", month: 5, reminder: "90 days · Jan 1", due: false, lane: "personal" },
        { id: "l5", title: "Visa renewal window opens", short: "Visa window", sub: "3 months before expiry · immigration bureau", date: "Jun 14", month: 7.45, reminder: "30 days · May 15", due: false, lane: "residence" },
        { id: "l6", title: "Residence card expires", short: "Residence card", sub: "renew before this date", date: "Sep 14", month: 10.45, reminder: "90 days · Jun 16", due: false, lane: "residence" },
      ],
    }),
    weeklyReview: () => sample({
      title: "A steady week; sleep carried it", week: "Sunday, Oct 25 · week of Oct 19–25 · health, work and money",
      tiles: [
        { key: "Health", big: "7h 18m", unit: "avg sleep", delta: "▲ 22 min vs last week", up: true, sub: "HRV 59 ms (▲ 3) · 2 workouts of 4 · recovery avg 71%" },
        { key: "Work", big: "5", unit: "focus sessions", delta: "▲ 1 · avg focus 61", up: true, sub: "Taiyo Skin: 4 launch tasks done · 2 crew runs, 1 failed" },
        { key: "Money", big: "$412", unit: "personal spend", delta: "▲ $96 vs your usual week", up: false, sub: "Business net $1,580 / 30 days · 2 decisions waiting" },
      ],
      change: { title: "Move hard workouts to Friday and Saturday.", body: "Focus dropped to **57** the day after each hard workout, vs **63** otherwise. With the Taiyo Skin launch tasks due Mon–Thu, training late in the week protects your best focus days." },
      highlights: [
        { ok: true, title: "Asleep by 23:30 six nights out of seven", sub: "Streak 9 nights" },
        { ok: true, title: "Best session: focus 66 on Sat after 8.0 h sleep", sub: "Neuro history" },
        { ok: false, title: "Workouts 2 of 4", sub: "Goal: 4 a week" },
      ],
      notes: [{ text: "Supplier call ran late.", day: "Fri" }, { text: "Slept 8 hours. Best session in weeks.", day: "Sat" }],
    }),
    tryChange: async (take) => sample({ reminder: take ? "Reminder set for Friday 08:00: hard workout today" : null }),
  };
}

/** Rows as src-tauri life_dates.rs and checkin.rs return them. */
export type LifeDateWire = { id: string; title: string; date: string; lane: "personal" | "residence"; sub: string; inDays: number; reminders: Array<{ days: number; on: string; passed: boolean }>; activeWindow: number | null;
  leadDays?: number[]; done?: boolean; goalId?: string | null };
export type CheckInWire = { id: string; at: string; day: string; mood: number; energy: number; tags: string[]; text: string; transcribed: boolean };

const SHORT_MONTH = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const dayLabel = (iso: string) => SHORT_MONTH.format(new Date(`${iso}T00:00:00Z`));

/** Upcoming dates only (the page shows the next 12 months); `month` is months from today for the timeline. */
export function lifeAdminFrom(rows: LifeDateWire[]): LifeAdmin {
  const dates: LifeDate[] = rows.filter((r) => r.inDays >= 0).map((r) => {
    const next = r.reminders.find((x) => !x.passed) ?? r.reminders[r.reminders.length - 1];
    return {
      id: r.id, title: r.title, short: r.title, sub: r.sub || (r.lane === "residence" ? "Japan · residence" : "Personal"), date: dayLabel(r.date), iso: r.date, rawSub: r.sub,
      month: r.inDays / 30.4, inDays: r.inDays, reminder: r.done ? "Done" : next ? `${next.days} days · ${dayLabel(next.on)}` : "", due: r.activeWindow != null && !r.done, lane: r.lane,
      leadDays: r.leadDays ?? [], done: r.done ?? false, goalId: r.goalId ?? null,
    };
  });
  const first = dates.find((d) => !d.done);
  return { next: first ? `${first.title} in ${first.inDays} days` : "Nothing coming up", dates };
}

const WHEN = new Intl.DateTimeFormat("en-US", { weekday: "short", hour: "numeric", minute: "2-digit", timeZone: "Asia/Tokyo" });
export const checkInFrom = (r: CheckInWire): CheckIn => ({ id: r.id, when: WHEN.format(new Date(r.at)).replace(",", ""), mood: r.mood, energy: r.energy, tags: r.tags, text: r.text, transcribed: r.transcribed || undefined });

/** Row as src-tauri goals.rs `read_goals` returns it. Habits are `category === "habit"` (adds.rs). */
export type GoalWire = { id: string; title: string; category: string; target: number; current: number; unit: string; deadline?: string | null;
  created_at?: string; updated_at?: string;
  /** Habits only (0045): why it matters, `HH:MM` JST reminder, paused. Round-tripped by the goal store so a save never drops them. */
  why?: string | null; remind_at?: string | null; paused?: boolean;
  /** Link to an Enterprise venture (goal_venture_set). */ venture_id?: string | null };

const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : 0);
const isGoalWire = (g: unknown): g is GoalWire => { const o = g as Record<string, unknown> | null; return !!o && typeof o.id === "string" && typeof o.title === "string"; };
const deadlineLabel = (d: string) => (/^\d{4}-\d{2}-\d{2}/.test(d) ? dayLabel(d.slice(0, 10)) : d);
const deadlineTime = (d: string | null | undefined): number | null => {
  if (!d) return null;
  const t = Date.parse(/^\d{4}-\d{2}-\d{2}$/.test(d) ? `${d}T00:00:00Z` : d);
  return Number.isNaN(t) ? null : t;
};

/** A goal's roadmap progress, as the roadmap adapter reports it (steps done / total, weighted percent). */
export type RoadmapProgress = { pct: number; done: number; total: number };

/** A row of `habit_days` (src-tauri habits.rs): one habit, one JST day. */
export type HabitDayWire = { habitId: string; day: string; done: boolean; source: string; at: string };
/** The habit grid covers this many days ending today (the Heatmap's 12 weeks). */
export const HABIT_WINDOW_DAYS = 84;
const JST_DAY = new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Tokyo", year: "numeric", month: "2-digit", day: "2-digit" });
/** Today in JST as YYYY-MM-DD. */
export const jstToday = (now: Date = new Date()): string => JST_DAY.format(now);
/** `YYYY-MM-DD` plus `n` days (calendar arithmetic, no time zone involved). */
export const addDays = (day: string, n: number): string => new Date(Date.parse(`${day}T00:00:00Z`) + n * 86_400_000).toISOString().slice(0, 10);
const mondayOf = (day: string): string => addDays(day, -((new Date(`${day}T00:00:00Z`).getUTCDay() + 6) % 7));

/** One habit's grid from its marks. A done mark is "met", an explicit not-done is "missed", a past day on or after the
 *  day the habit was created with no mark is "missed" (not marked), today with no mark and any earlier day stay "none". */
export function habitDays(g: GoalWire, rows: HabitDayWire[], today: string): { days: DayMark[]; done: number; todayState: "done" | "not" | "open" } {
  const mine = new Map(rows.filter((r) => r.habitId === g.id).map((r) => [r.day, r.done]));
  const created = typeof g.created_at === "string" && /^\d{4}-\d{2}-\d{2}/.test(g.created_at) ? g.created_at.slice(0, 10) : null;
  const days: DayMark[] = [];
  for (let i = HABIT_WINDOW_DAYS - 1; i >= 0; i--) {
    const d = addDays(today, -i);
    const m = mine.get(d);
    days.push(m === true ? "met" : m === false ? "missed" : d < today && created !== null && d >= created && !g.paused ? "missed" : "none");
  }
  const week = mondayOf(today);
  const done = [...mine].filter(([d, v]) => v && d >= week && d <= today).length;
  const t = mine.get(today);
  return { days, done, todayState: t === true ? "done" : t === false ? "not" : "open" };
}

/** Goals and habits from `read_goals`. Habit days stay "none": no source data is linked to them, so nothing is ever met or missed.
 *  A goal that has a roadmap shows the roadmap's percent and steps done (one progress meaning, as on the Goals page); the stale
 *  `current` / `target` only speaks for a goal with no roadmap. */
export function goalsFrom(rows: GoalWire[], roadmapOf?: (goalId: string) => RoadmapProgress | null, habitMarks?: HabitDayWire[] | null, today: string = jstToday()): GoalsWeek {
  const clamp = (n: number) => Math.max(0, Math.min(100, n));
  const rowsOk = rows.filter(isGoalWire);
  const items = rowsOk.filter((g) => g.category !== "habit").map((g) => {
    const target = num(g.target), current = num(g.current), unit = typeof g.unit === "string" ? g.unit.trim() : "";
    const rm = roadmapOf?.(g.id) ?? null;
    const pct = rm ? Math.round(clamp(rm.pct)) : target > 0 ? Math.round(clamp((current / target) * 100)) : 0;
    const right = rm ? `${pct}% · ${rm.done} of ${rm.total} steps` : unit === "%" ? `${pct}%` : target > 0 ? `${current} of ${target}${unit ? ` ${unit}` : ""}` : `${current}${unit ? ` ${unit}` : ""}`;
    const dl = g.deadline ? deadlineLabel(g.deadline) : "";
    const m: Milestone = { id: g.id, title: g.title, detail: [g.category, dl].filter(Boolean).join(" · "), right, pct };
    return { m, t: deadlineTime(g.deadline) };
  });
  const open = items.filter((x) => x.m.pct < 100);
  const dated = open.filter((x) => x.t !== null).sort((a, b) => (a.t as number) - (b.t as number));
  const next = dated[0] ?? [...open].sort((a, b) => b.m.pct - a.m.pct)[0];
  const habits: Habit[] = rowsOk.filter((g) => g.category === "habit").map((g) => {
    const unit = typeof g.unit === "string" ? g.unit : "";
    const t = num(g.target);
    const target = t > 0 ? Math.min(7, Math.round(t)) : 7;
    const extra = { ...(typeof g.why === "string" && g.why ? { why: g.why } : {}), ...(typeof g.remind_at === "string" && g.remind_at ? { remindAt: g.remind_at } : {}), ...(g.paused ? { paused: true } : {}) };
    if (!habitMarks) return { id: g.id, title: g.title, source: "You", rule: unit, days: Array.from({ length: 7 }, () => "none" as DayMark), done: 0, target, note: "No data source linked yet", ...extra };
    const h = habitDays(g, habitMarks, today);
    return { id: g.id, title: g.title, source: "You", rule: unit, days: h.days, done: h.done, target, note: `${h.done} of ${target} this week`, today: h.todayState, ...extra };
  });
  return { habits, milestones: items.map((x) => x.m), nextMilestone: next ? next.m.title : null };
}

const BLOCK_CATEGORY: Record<string, MemoryCategory> = { human: "about", persona: "preferences" };
const isMemoryWire = (m: unknown): m is MemoryWire => { const o = m as Record<string, unknown> | null; return !!o && typeof o.id === "string" && typeof o.value === "string"; };

/** Letta core-memory blocks as Memory rows. Only the `human` and `persona` labels have a known category; the rest read as "About you". */
export function memoriesFrom(rows: MemoryWire[]): Memory[] {
  // A cleared block (what "Delete" leaves behind) has no text left to show.
  return rows.filter(isMemoryWire).filter((m) => m.value.trim() !== "").map((m) => {
    const label = typeof m.label === "string" && m.label ? m.label : "block";
    const agent = typeof m.agent === "string" && m.agent ? m.agent : "Amara";
    const when = typeof m.updatedAt === "string" && /^\d{4}-\d{2}-\d{2}/.test(m.updatedAt) ? ` · updated ${dayLabel(m.updatedAt.slice(0, 10))}` : "";
    return { id: m.id, text: m.truncated ? `${m.value} …` : m.value, category: BLOCK_CATEGORY[label] ?? "about", source: `${agent} · ${label} block${when}` };
  });
}

/** Fixed text per memory_list error code. Nothing from the error itself is shown. */
export function memoryNote(e: unknown): string {
  const code = typeof e === "string" ? e : "";
  if (code === "letta_no_key") return "Add the Letta password in Settings › Secrets, then Amara's memory shows here. Nothing was read.";
  if (code === "letta_off") return "Letta isn't answering on this Mac. Start the stack, then reopen this page. Nothing was read.";
  if (code === "letta_unauthorized") return "Letta refused the saved password. Save it again in Settings › Secrets. Nothing was read.";
  return "Letta sent something Amara couldn't read. Nothing was changed.";
}

/** Local YYYY-MM-DD `days` from `now`. */
export function localDatePlus(now: Date, days: number): string {
  const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() + days);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Weekly review › "Try it this week" / "Not this week". Only a real suggestion can be tried; the follow-up add asks
 *  first (native dialog), and a cancel or error says nothing was set. Goals are never touched. */
export async function tryChangeWith(call: Invoke, take: boolean, suggestion: string | null, now = new Date(), pair: string | null = null, why: { reason?: string | null; findingId?: string | null } = {}): Promise<Wired<{ reminder: string | null }>> {
  if (!suggestion) return UNWIRED;
  if (!take) return live({ reminder: null });
  const due = localDatePlus(now, 6);
  try {
    // The follow-up keeps its reason and the finding behind it, so Needs you can say why and Patterns is one step away.
    const reason = (why.reason ?? "").replace(/\s+/g, " ").trim().slice(0, 240);
    const next = [reason && `Why: ${reason}`, why.findingId ? `Finding: ${why.findingId} (Mind–Body › Patterns)` : ""].filter(Boolean).join(" · ") || null;
    const added = await call<{ id?: unknown }>("followups_add_item", { text: `Try this week: ${suggestion}`, due, next });
    const reminder = `Follow-up added for ${due}: ${suggestion}`;
    // Track the try-it as an experiment (sidecar in Rust; the follow-up itself gets no new fields). `pair` is the finding behind
    // the suggestion; with none, Rust stores no measure and the result says so. A failure here never undoes the follow-up.
    if (typeof added?.id === "number") {
      try { await call("experiments_record_start", { followupId: added.id, pair }); }
      catch { /* silent to the text: copy changes need Design (tasks/design-questions.md, 2026-11-14) */ }
    }
    return live({ reminder });
  } catch {
    return live({ reminder: "Nothing was set (cancelled or couldn't save)" });
  }
}

/** Fixed text per memory_edit / memory_delete / memory_restore_last error code. Nothing from the error itself is shown. */
export function memoryWriteNote(e: unknown, what: "Edit" | "Delete" | "Undo"): string {
  const code = typeof e === "string" ? e : "";
  if (code === "cancelled") return "Cancelled. Nothing was changed.";
  if (code === "letta_no_key") return "Add the Letta password in Settings › Secrets first. Nothing was changed.";
  if (code === "letta_off") return "Letta isn't answering on this Mac. Nothing was changed.";
  if (code === "letta_unauthorized") return "Letta refused the saved password. Nothing was changed.";
  if (code === "letta_value_too_long") return "That memory is too long (2000 characters at most). Nothing was changed.";
  if (code === "letta_block_too_long") return "This memory block is longer than the app shows, so it can't be edited here without losing text. Nothing was changed.";
  if (code === "letta_bad_input") return "That memory can't be empty or changed this way. Nothing was changed.";
  if (code === "letta_backup_failed") return "Amara couldn't save the old text first, so nothing was changed.";
  if (code === "changed") return "That memory changed while you were deciding, so nothing was written. Reopen Memory and try again.";
  if (code === "letta_busy") return "Another memory change is still open. Finish or cancel it first. Nothing was changed.";
  if (code === "letta_unconfirmed") return "It may or may not have changed — reopen Memory to check.";
  if (code === "letta_no_backup") return "There's no earlier version of this memory to put back.";
  return `${what} didn't go through. It may or may not have changed — reopen Memory to check.`;
}

/**
 * The stores built so far (life dates, check-ins, goals, Letta memory blocks: read, edit, clear, undo). Everything else stays unwired. Each reads as unwired until its
 * first successful read, so a missing backend never shows an empty live list.
 */
/** Steps done / total and percent from the roadmap adapter's cache; null while the goal has no live steps (then the goal's own numbers speak). */
function roadmapProgress(goalId: string): RoadmapProgress | null {
  const w = roadmapAdapter().roadmap(goalId);
  if (w.state === "unwired" || !w.value || w.value.steps.length === 0) return null;
  return { pct: w.value.pct, done: w.value.steps.filter((x) => x.done).length, total: w.value.steps.length };
}

export function makeLivePersonal(call: Invoke = realInvoke): PersonalAdapter & { refresh(): Promise<void> } {
  let admin: LifeAdmin | null = null;
  let checkins: CheckIn[] | null = null;
  let goalRows: GoalWire[] | null = null;
  let habitMarks: HabitDayWire[] | null = null;
  let patterns: Patterns | null = null;
  let review: WeeklyReview | null = null;
  let suggestionTitle: string | null = null;
  let suggestionPair: string | null = null; // from the finding id: "<pair>" or "tag_mood:<tag>"
  let suggestionWhy: { reason: string | null; findingId: string | null } = { reason: null, findingId: null };
  let memories: Memory[] | null = null;
  let money: PersonalMoney | null = null;
  let memoriesNote: string | null = null;
  let writeNote: string | null = null;
  const refresh = async () => {
    // Each store settles on its own: a malformed reply (a mapper that throws) clears only that store.
    await Promise.allSettled([
      call<LifeDateWire[]>("life_date_list").then((r) => { admin = lifeAdminFrom(r); }).catch(() => { admin = null; }),
      call<CheckInWire[]>("checkin_list", { limit: 30 }).then((r) => { checkins = r.map(checkInFrom); }).catch(() => { checkins = null; }),
      call<PatternsWire>("patterns_read").then((r) => { patterns = r && Array.isArray(r.findings) ? patternsFrom(r) : null; }).catch(() => { patterns = null; }),
      call<ReviewWire>("weekly_review_read")
        .then((r) => { review = r && r.health ? weeklyReviewFrom(r) : null; suggestionTitle = review && r.suggestion ? r.suggestion.title : null; suggestionPair = review && r.suggestion?.evidenceFindingId ? r.suggestion.evidenceFindingId.split(":")[0] : null; suggestionWhy = { reason: r.suggestion?.reason ?? null, findingId: r.suggestion?.evidenceFindingId ?? null }; })
        .catch(() => { review = null; suggestionTitle = null; suggestionPair = null; suggestionWhy = { reason: null, findingId: null }; }),
      Promise.all([call<RawSummary | null>("money_summary_read"), call<{ rate: number; date: string; source: string } | null>("fx_rate_get")])
        .then(([sm, f]) => { money = personalMoneyFrom(sm && Array.isArray(sm.recurring) ? sm : null, mapFx(f)); }).catch(() => { money = null; }),
      call<{ checkin?: { at?: string } }>("proactive_settings_get").then((r) => { setCheckInReminderAt(r?.checkin?.at); }).catch(() => {}),
      call<GoalWire[]>("read_goals").then((r) => { goalRows = Array.isArray(r) ? r : null; }).catch(() => { goalRows = null; }),
      // A habit log that can't be read leaves the marks unknown (the grid stays empty), never a made-up miss.
      (() => { const t = jstToday(); return call<HabitDayWire[]>("habit_days", { from: addDays(t, -(HABIT_WINDOW_DAYS - 1)), to: t }); })()
        .then((r) => { habitMarks = Array.isArray(r) ? r : null; }).catch(() => { habitMarks = null; }),
      call<MemoryWire[]>("memory_list").then(
        (r) => { memories = Array.isArray(r) ? memoriesFrom(r) : null; memoriesNote = memories ? null : memoryNote(null); },
        (e) => { memories = null; memoriesNote = memoryNote(e); },
      ),
    ]);
    announceStores();
  };
  const writeMemory = async (cmd: string, args: Record<string, unknown>, what: "Edit" | "Delete" | "Undo", okNote: string): Promise<MemoryWrite> => {
    writeNote = null;
    try {
      await call<void>(cmd, args);
      writeNote = okNote;
      await refresh();
      return live({ ok: true as const });
    } catch (e) {
      writeNote = memoryWriteNote(e, what);
      return UNWIRED;
    }
  };
  try { window.addEventListener(ADDS_EVENT, () => { void refresh(); }); } catch { /* no window in node tests */ }
  // inbox.rs applied a Telegram reply (check-in, habit or money): re-read, so open pages show it without a restart.
  try { void listen("amara-inbox://applied", () => { void refresh(); announceAdds(); announceSnapshot(); }).catch(() => {}); } catch { /* no Tauri event bridge (tests, browser preview) */ }
  return {
    ...unwiredPersonal,
    refresh,
    // Read at call time so a step finished on the roadmap moves the goal here without waiting for a re-read of goals.json.
    goals: () => (goalRows ? live(goalsFrom(goalRows, roadmapProgress, habitMarks)) : UNWIRED),
    money: () => (money ? live(money) : UNWIRED),
    patterns: () => (patterns ? live(patterns) : UNWIRED),
    weeklyReview: () => (review ? live(review) : UNWIRED),
    memories: () => (memories ? live([...memories]) : UNWIRED),
    memoriesNote: () => memoriesNote,
    memoryWriteNote: () => writeNote,
    // Edit saves the old text first; "delete" empties the Letta block (the block stays) after a native approval dialog. No source for pendingMemories / decideMemory.
    editMemory: (id, text) => writeMemory("memory_edit", { blockId: id, value: text }, "Edit", "Saved. The old text is kept on this Mac, so it can be put back."),
    deleteMemory: (id) => writeMemory("memory_delete", { blockId: id }, "Delete", "Cleared. The old text is kept on this Mac, so it can be put back."),
    restoreMemory: (id) => writeMemory("memory_restore_last", { blockId: id }, "Undo", "Put back the earlier text."),
    lifeAdmin: () => (admin ? live(admin) : UNWIRED),
    // "Try it this week" adds one follow-up (native approval dialog in Rust), due in 6 days; it never changes a goal.
    // "Not this week" stores nothing. With no real suggestion from the weekly review there's nothing to try.
    tryChange: async (take) => tryChangeWith(call, take, suggestionTitle, new Date(), suggestionPair, suggestionWhy),
    checkins: () => (checkins ? live([...checkins]) : UNWIRED),
    // On-device dictation via Amara Voice (voice.rs dictate_once): text only. Unwired until the patched loop is ready.
    transcribe: async () => { const t = await dictateOnce("checkin", call); return t ? live(t) : UNWIRED; },
    saveCheckIn: async (d) => {
      if (d.mood == null || d.energy == null) return UNWIRED;
      try {
        const r = await call<CheckInWire>("checkin_save", { draft: { mood: d.mood, energy: d.energy, tags: d.tags, text: d.text } });
        await refresh();
        return live(checkInFrom(r));
      } catch { return UNWIRED; }
    },
  };
}

let sampleSingleton: PersonalAdapter | null = null;
let liveSingleton: ReturnType<typeof makeLivePersonal> | null = null;
export const personalAdapter = (): PersonalAdapter => {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSamplePersonal());
  if (!inTauri()) return unwiredPersonal;
  if (!liveSingleton) { liveSingleton = makeLivePersonal(); void liveSingleton.refresh(); }
  return liveSingleton;
};
/** Test seam. */
export const resetLivePersonal = () => { liveSingleton = null; };

/** design-1111v: Needs you today drops its quiet check-in row when a check-in is saved. */
export const CHECKIN_EVENT = "amara:checkin";

/** Built defaults for Taisun's open 1111a choices (HANDOFF-1111a "Built"). */
let checkInAt = "22:00";
/** The saved Telegram check-in time (proactive_settings_get); the built default 22:00 until it is read. */
export const setCheckInReminderAt = (hhmm: unknown) => { if (typeof hhmm === "string" && /^([01]\d|2[0-3]):[0-5]\d$/.test(hhmm)) checkInAt = hhmm; };
export const PERSONAL_DEFAULTS = {
  get checkInReminder() { return `${checkInAt} JST`; },
  weeklyReview: "Sunday 19:00 JST",
  tryItSetsReminder: true,
  confirmMemories: ["health", "people"] as MemoryCategory[],
  lifeAdminLadder: [90, 30, 7],
};

/** True on Sunday in Tokyo: the weekly review moves to the top of Today. */
export function isTokyoSunday(now: Date): boolean {
  return new Intl.DateTimeFormat("en-US", { weekday: "short", timeZone: "Asia/Tokyo" }).format(now) === "Sun";
}
