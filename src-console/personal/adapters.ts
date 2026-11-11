import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";

/**
 * design-1111a: Amara as a personal OS. Every surface reads from this adapter. In the app it is
 * unwired ("Needs wiring") until Claude builds the stores and commands in HANDOFF-1111a.md; the
 * sample adapter is preview/test only and every value it returns carries a "Sample data" tag.
 * Real visa dates, money accounts and the JPY→USD rule come from Taisun later: never invented here.
 */

// ---- Goals and habits ----
export type DayMark = "met" | "nearly" | "missed" | "none";
export type Habit = { id: string; title: string; source: string; rule: string; days: DayMark[]; done: number; target: number; note: string };
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

// ---- Life admin ----
export type LifeDate = { id: string; title: string; sub: string; date: string; month: number; inDays?: number; reminder: string; due: boolean; lane: "residence" | "work" };
export type LifeAdmin = { next: string; dates: LifeDate[] };

// ---- Weekly review ----
export type WeeklyReview = {
  title: string; week: string;
  tiles: Array<{ key: string; big: string; unit: string; delta: string; up: boolean; sub: string }>;
  change: { title: string; body: string };
  highlights: Array<{ ok: boolean; title: string; sub: string }>;
  notes: Array<{ text: string; day: string }>;
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
  pendingMemories(): Wired<PendingMemory[]>;
  editMemory(id: string, text: string): Promise<MemoryWrite>;
  deleteMemory(id: string): Promise<MemoryWrite>;
  decideMemory(id: string, keep: boolean): Promise<MemoryWrite>;
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
      next: "Taiyo Skin launch in 24 days",
      dates: [
        { id: "l1", title: "Taiyo Skin launch", sub: "Business › Ventures", date: "Nov 21", month: 0.77, inDays: 24, reminder: "7 days · Nov 14", due: true, lane: "work" },
        { id: "l2", title: "Akiya offer deadline", sub: "Kamakura listing · offer + deposit", date: "Dec 18", month: 1.65, inDays: 51, reminder: "30 days · Nov 18", due: true, lane: "work" },
        { id: "l3", title: "Taiyo Skin tax filing", sub: "with the accountant", date: "Feb 15", month: 3.57, reminder: "90 days · Nov 17", due: false, lane: "work" },
        { id: "l4", title: "Apartment contract renewal", sub: "2-year lease · notice 1 month before", date: "Apr 1", month: 5.1, reminder: "90 days · Jan 1", due: false, lane: "residence" },
        { id: "l5", title: "Visa renewal window opens", sub: "3 months before expiry · immigration bureau", date: "Jun 14", month: 7.55, reminder: "30 days · May 15", due: false, lane: "residence" },
        { id: "l6", title: "Residence card expires", sub: "renew before this date", date: "Sep 14", month: 10.55, reminder: "90 days · Jun 16", due: false, lane: "residence" },
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

let sampleSingleton: PersonalAdapter | null = null;
export const personalAdapter = (): PersonalAdapter => (sampleAdaptersEnabled() ? (sampleSingleton ??= makeSamplePersonal()) : unwiredPersonal);

/** Built defaults for Taisun's open 1111a choices (HANDOFF-1111a "Built"). */
export const PERSONAL_DEFAULTS = {
  checkInReminder: "22:00 JST",
  weeklyReview: "Sunday 19:00 JST",
  tryItSetsReminder: true,
  confirmMemories: ["health", "people"] as MemoryCategory[],
  lifeAdminLadder: [90, 30, 7],
};

/** True on Sunday in Tokyo: the weekly review moves to the top of Today. */
export function isTokyoSunday(now: Date): boolean {
  return new Intl.DateTimeFormat("en-US", { weekday: "short", timeZone: "Asia/Tokyo" }).format(now) === "Sun";
}
