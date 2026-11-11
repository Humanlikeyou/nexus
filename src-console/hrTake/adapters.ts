import { sample, sampleAdaptersEnabled, UNWIRED, type Wired } from "../lib/sampleAdapters";

/**
 * design-1111u: Home's take from the last 15 minutes of heart rate. Taisun (Nov 11, 16:04 / 17:36 JST):
 * no button and no countdown. A new take is written automatically when he opens or comes back to Home and the
 * last take is 30+ minutes old. A take needs readings in 10 of the 15 minutes, and WHOOP counts as down after
 * 10 minutes without a sync. "Your usual" is the same time of day over the last 14 days.
 *
 * The 30-minute rule lives in the backend (docs/design/HANDOFF-1111u.md §4). The UI never decides when a take is
 * written; it only asks on open, focus and navigation, and shows what the backend says. In the app the take is unwired
 * ("Needs wiring") until `home_hr_take_status` / `home_hr_take_maybe_refresh` are registered and wired via `liveOf`. The sample adapter is preview/test only and
 * every value it returns carries a "Sample data" tag. It mocks the 30-minute rule so the states can be seen.
 */

export const HR_WINDOW_MIN = 15;
export const HR_MIN_COVERED = 10;
export const HR_SOURCE_STALE_MS = 10 * 60_000;
/** The backend's gate. The UI uses it only to label age; it never gates on it. */
export const HR_TAKE_GAP_MS = 30 * 60_000;

export type HrMinute = { t: number; bpm: number | null };
export type HrStats = { avg: number; min: number; max: number; first: number; last: number };
export type HrUsual = { lo: number; hi: number; days: number };
export type HrWindow = {
  from: number; to: number; source: "whoop" | "other"; lastSampleAt: number | null;
  perMinute: HrMinute[]; coveredMin: number; stats: HrStats | null; usual: HrUsual | null;
};
export type HrTake = { id: string; generatedAt: number; window: HrWindow; text: string; memory: string | null; question: string };

export type HrTrigger = "open" | "focus" | "navigate";
/** What the backend reports. `due`: the last take is 30+ minutes old (or there is none), so a refresh may write one. */
export type HrTakeStatus =
  | { kind: "take"; take: HrTake; due: boolean }
  | { kind: "none_yet"; due: boolean }
  | { kind: "not_enough"; previous: HrTake | null; window: HrWindow; due: boolean }
  | { kind: "source_down"; previous: HrTake | null; lastSampleAt: number | null; due: boolean };

export interface HrTakeAdapter {
  /** Read only: the newest take and whether one is due. Never writes. */
  status(): Promise<Wired<HrTakeStatus>>;
  /** Ask the backend to write a take if its 30-minute gate allows. Concurrent calls must make at most one take. */
  refresh(trigger: HrTrigger): Promise<Wired<HrTakeStatus>>;
}

// ---- Pure helpers (shared with the backend contract and the template fallback) ----

export function windowStats(perMinute: readonly HrMinute[]): HrStats | null {
  const v = perMinute.map((m) => m.bpm).filter((b): b is number => typeof b === "number" && Number.isFinite(b));
  if (!v.length) return null;
  return { avg: Math.round(v.reduce((a, b) => a + b, 0) / v.length), min: Math.min(...v), max: Math.max(...v), first: v[0], last: v[v.length - 1] };
}
export const coveredMinutes = (perMinute: readonly HrMinute[]) => Math.min(HR_WINDOW_MIN, perMinute.filter((m) => m.bpm != null).length);

export type HrTrend = "rising" | "falling" | "steady";
export function trendOf(s: HrStats): HrTrend { const d = s.last - s.first; return d >= 5 ? "rising" : d <= -5 ? "falling" : "steady"; }
export type HrPlace = "inside" | "above" | "below";
export function placeOf(s: HrStats, u: HrUsual): HrPlace { return s.avg > u.hi ? "above" : s.avg < u.lo ? "below" : "inside"; }

/** Can a take be written from this window? (The backend applies the same rule.) */
export function windowBlock(w: HrWindow, now: number): "source_down" | "not_enough" | null {
  if (w.lastSampleAt == null || now - w.lastSampleAt > HR_SOURCE_STALE_MS) return "source_down";
  if (w.coveredMin < HR_MIN_COVERED || !w.stats) return "not_enough";
  return null;
}

/** The numbers a take may use. Anything else in the text is rejected (HANDOFF-1111u §5). */
export function hrFacts(w: HrWindow): Set<number> {
  const s = w.stats; const out = new Set<number>([HR_WINDOW_MIN, w.coveredMin]);
  if (s) for (const n of [s.avg, s.min, s.max, s.first, s.last, Math.abs(s.last - s.first)]) out.add(n);
  if (w.usual) { out.add(w.usual.lo); out.add(w.usual.hi); }
  return out;
}
const CAUSAL = /\b(caus\w*|prove[sdn]?|proof that|because of|fixed|cured?|guarantee\w*|diagnos\w*|arrhythmi\w*)\b/i;
/** True when every number in the text is a fact and no causal or medical words are used. Times ("2:00") are ignored. */
export function hrTextOk(text: string, w: HrWindow): boolean {
  if (CAUSAL.test(text)) return false;
  const facts = hrFacts(w);
  const plain = text.replace(/\d{1,2}:\d{2}(\s?[AP]M)?/g, " ");
  return (plain.match(/\d+/g) ?? []).every((n) => facts.has(Number(n)));
}

/** The template take: what Amara says when the local model is unavailable or its text fails the check. **x** = bold. */
export function hrTemplate(w: HrWindow): string {
  const s = w.stats; if (!s) return "";
  const t = trendOf(s);
  const first = t === "falling" ? `Your heart rate eased from **${s.first}** to **${s.last}** over the last 15 minutes, averaging **${s.avg}**.`
    : t === "rising" ? `Your heart rate rose from **${s.first}** to **${s.last}** over the last 15 minutes, averaging **${s.avg}**.`
    : `Your heart rate held steady around **${s.avg}** over the last 15 minutes (**${s.min}–${s.max}**).`;
  if (!w.usual) return first;
  const p = placeOf(s, w.usual); const band = `your usual for this time of day (**${w.usual.lo}–${w.usual.hi}**)`;
  const second = p === "inside" ? `That's inside ${band}: a calm stretch that may suit focused work.`
    : p === "above" ? `That's above ${band}. If you've been sitting, a few slow breaths or a short walk may help.`
    : `That's below ${band}, a quiet stretch.`;
  return `${first} ${second}`;
}

export const tokyoTime = (ms: number) => new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", timeZone: "Asia/Tokyo" });

/** The memory line: a template over the previous stored take, never model recall. */
export function hrMemoryLine(prev: HrTake | null): string | null {
  const s = prev?.window.stats; if (!prev || !s) return null;
  const t = trendOf(s);
  return `At **${tokyoTime(prev.generatedAt)}** you were at **${s.last}** and ${t === "rising" ? "climbing" : t === "falling" ? "settling" : "steady"}.`;
}

/** Age, quietly: "Just now" under a minute, otherwise "as of 2:00 PM · 18 min ago". */
export function ageLabel(generatedAt: number, now: number): string {
  const min = Math.floor(Math.max(0, now - generatedAt) / 60_000);
  if (min < 1) return "Just now";
  const ago = min < 60 ? `${min} min ago` : `${Math.floor(min / 60)} h ${min % 60} min ago`;
  return `as of ${tokyoTime(generatedAt)} · ${ago}`;
}

// ---- Shape check for backend answers (anything else = unwired) ----
const num = (v: unknown) => typeof v === "number" && Number.isFinite(v);
function isWindow(w: unknown): w is HrWindow {
  const x = w as HrWindow;
  return !!x && typeof x === "object" && num(x.from) && num(x.to) && Array.isArray(x.perMinute) && num(x.coveredMin) && (x.lastSampleAt === null || num(x.lastSampleAt));
}
function isTake(t: unknown): t is HrTake {
  const x = t as HrTake;
  return !!x && typeof x === "object" && typeof x.id === "string" && num(x.generatedAt) && typeof x.text === "string" && isWindow(x.window);
}
export function isHrStatus(v: unknown): v is HrTakeStatus {
  const x = v as HrTakeStatus;
  if (!x || typeof x !== "object" || typeof x.due !== "boolean") return false;
  switch (x.kind) {
    case "take": return isTake(x.take);
    case "none_yet": return true;
    case "not_enough": return (x.previous === null || isTake(x.previous)) && isWindow(x.window);
    case "source_down": return (x.previous === null || isTake(x.previous)) && (x.lastSampleAt === null || num(x.lastSampleAt));
    default: return false;
  }
}

/**
 * The app: unwired ("Needs wiring") until Claude registers the two commands (HANDOFF-1111u §10). Frontend code may
 * only invoke registered commands (test/commands.registered.test.ts), so the wiring is:
 *   status  → the home_hr_take_status command, answer passed through liveOf
 *   refresh → the home_hr_take_maybe_refresh command with { trigger }, answer passed through liveOf
 * No numbers are ever made up here.
 */
export const liveHrTake: HrTakeAdapter = {
  async status() { return UNWIRED; },
  async refresh() { return UNWIRED; },
};
/** For the wiring: a backend answer becomes live only when it has the right shape. */
export const liveOf = (v: unknown): Wired<HrTakeStatus> => (isHrStatus(v) ? { state: "live", value: v } : UNWIRED);

// ---- Sample adapter (preview/test only, labelled) ----

/** Preview flags (localStorage "nexus.hrTakePreview", JSON), read only in sample mode. */
export const HR_TAKE_PREVIEW_KEY = "nexus.hrTakePreview";
export type HrScenario = "first" | "current" | "regenerating" | "new" | "not_enough" | "source_down" | "unwired";
export type HrPreview = { scenario?: HrScenario; ageMin?: number; delayMs?: number };

// Sample 15-minute windows, one reading a minute. Invented, never shown without the Sample data tag.
const CALM = [79, 81, 80, 78, 77, 76, 75, 74, 72, 71, 70, 69, 68, 67, 68];
const CLIMB = [70, 71, 73, 76, 80, 84, 86, 88, 87, 85, 84, 83, 82, 82, 80];
const EARLIER = [76, 77, 78, 79, 80, 80, 81, 82, 82, 83, 83, 84, 84, 85, 84];
const GAPPY: Array<number | null> = [72, 71, 73, null, null, null, null, null, null, null, null, null, 74, 75, 74];
const SAMPLE_USUAL: HrUsual = { lo: 62, hi: 74, days: 14 };

export function sampleWindow(to: number, bpm: ReadonlyArray<number | null>, lastSampleAt: number | null = to): HrWindow {
  const perMinute = bpm.map((b, i) => ({ t: to - (bpm.length - 1 - i) * 60_000, bpm: b }));
  return { from: to - HR_WINDOW_MIN * 60_000, to, source: "whoop", lastSampleAt, perMinute, coveredMin: coveredMinutes(perMinute), stats: windowStats(perMinute), usual: SAMPLE_USUAL };
}
function sampleTakeAt(at: number, bpm: readonly number[], prev: HrTake | null): HrTake {
  const window = sampleWindow(at, bpm);
  return { id: `sample-${at}`, generatedAt: at, window, text: hrTemplate(window), memory: hrMemoryLine(prev), question: "What's behind my heart rate right now?" };
}

/** A mock of the backend: one stored take, the 30-minute gate and an in-flight lock. */
export function sampleHrTake(opts: HrPreview = {}, clock: () => number = Date.now): HrTakeAdapter {
  const scenario = opts.scenario ?? "first";
  const t0 = clock();
  const age = (opts.ageMin ?? (scenario === "current" ? 18 : 47)) * 60_000;
  const earlier = scenario === "first" ? null : sampleTakeAt(t0 - age, CALM, sampleTakeAt(t0 - age - 48 * 60_000, EARLIER, null));
  let last: HrTake | null = earlier;
  let blocked: HrTakeStatus | null = null;
  let inflight: Promise<Wired<HrTakeStatus>> | null = null;
  const due = () => !last || clock() - last.generatedAt >= HR_TAKE_GAP_MS;
  const now = (): HrTakeStatus => blocked ?? (last ? { kind: "take", take: last, due: due() } : { kind: "none_yet", due: true });
  const delay = opts.delayMs ?? 900;
  return {
    async status() { return scenario === "unwired" ? UNWIRED : sample(now()); },
    refresh() {
      if (scenario === "unwired") return Promise.resolve(UNWIRED);
      if (!due()) return Promise.resolve(sample(now()));
      inflight ??= new Promise<Wired<HrTakeStatus>>((resolve) => {
        if (scenario === "regenerating") return; // preview: stays on "Reading…"
        setTimeout(() => {
          const at = clock();
          if (scenario === "not_enough") blocked = { kind: "not_enough", previous: last, window: sampleWindow(at, GAPPY), due: true };
          else if (scenario === "source_down") blocked = { kind: "source_down", previous: last, lastSampleAt: at - 87 * 60_000, due: true };
          else last = sampleTakeAt(at, scenario === "first" ? CALM : CLIMB, last);
          inflight = null;
          resolve(sample(now()));
        }, delay);
      });
      return inflight;
    },
  };
}

function previewOpts(): HrPreview {
  try { const raw = globalThis.localStorage?.getItem(HR_TAKE_PREVIEW_KEY); return raw ? JSON.parse(raw) as HrPreview : {}; } catch { return {}; }
}
let cached: HrTakeAdapter | null = null;
export function hrTakeAdapter(): HrTakeAdapter {
  if (!sampleAdaptersEnabled()) return liveHrTake;
  return (cached ??= sampleHrTake(previewOpts()));
}
export function resetHrTakeAdapter() { cached = null; }
