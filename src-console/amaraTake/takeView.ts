import { sampleAdaptersEnabled } from "../lib/sampleAdapters";
import type { AmaraTake, TakePage, TakeResult } from "./types";
import type { TakeDeps } from "./generate";

/** design-1110f: what the card shows. Lane colours a chip (Mind–Body: emerald = health, copper = neuro). */
export type TakeLane = "health" | "neuro" | "business";
export type TakeChip = { label: string; value: string; lane?: TakeLane; sourceId?: string };
export type TakeNote = { text: string; chips: TakeChip[]; time: string; sourceLine: string; lane?: string; question: string };
export type TakeView =
  | { state: "fresh"; note: TakeNote; sample: boolean }
  | { state: "generating" }
  | { state: "nodata"; waitingFor: string; sample: boolean }
  | { state: "failed"; reason: string }
  | { state: "unwired"; reason?: string };

export const TAKE_PAGE_LABEL: Record<TakePage, string> = { home: "Home", fitness: "Fitness", neuro: "Neuro", mindBody: "Mind–Body", business: "Enterprise", life: "Life", agents: "Agents", smarthome: "Smart home" };

/** What each page waits for before Amara writes (HANDOFF-1110f triggers). */
export const WAITING_FOR: Record<TakePage, string> = {
  home: "Amara writes this once today's sleep or a session lands.",
  fitness: "Amara writes this once last night's sleep syncs.",
  neuro: "Amara writes this once a Muse session ends.",
  mindBody: "Amara writes this once last night's sleep syncs.",
  business: "Amara writes this once today's business snapshot loads.",
  life: "Amara writes this once a habit, a date or this week's review has something to say.",
  agents: "Amara writes this once the Agents page can read her status.",
  smarthome: "Amara writes this once Home Assistant lists a device.",
};

const tokyoTime = (ms: number) => new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", timeZone: "Asia/Tokyo" });

/**
 * Where a cited number lives on its page (1110f onCite): the element id the chip scrolls to. Only
 * ids that really exist on that page are listed; a fact with no source element stays inert.
 */
export const CITE_TARGETS: Partial<Record<TakePage, Record<string, string>>> = {
  mindBody: { hrv: "mb-readiness", recovery: "mb-readiness", rhr: "mb-readiness", sleep: "mb-sleep" },
  fitness: { sleep: "sleep-card" },
  home: { recovery: "home-recovery-title", hrv: "home-recovery-title", sleep: "home-recovery-title" },
};

export const citeTarget = (page: TakePage, key: string): string | undefined => CITE_TARGETS[page]?.[key];

/** Scrolls to a cited number's source if that element is on the page now. Returns whether it did. */
export function goToCite(sourceId: string | undefined, doc: Document = document): boolean {
  const el = sourceId ? doc.getElementById(sourceId) : null;
  if (!el) return false;
  const reduce = doc.documentElement.dataset.reduceMotion === "true";
  el.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
  return true;
}

/** A live take from useAmaraTake: the text as generated and one chip per cited data point. */
export function liveNote(take: AmaraTake): TakeNote {
  return {
    text: take.text,
    chips: take.cited.map((c) => ({ label: c.label, value: c.display, sourceId: citeTarget(take.page, c.key) })),
    time: `Today, ${tokyoTime(take.generatedAt)}`,
    sourceLine: `From today's ${TAKE_PAGE_LABEL[take.page]} data`,
    question: `What's behind today's ${TAKE_PAGE_LABEL[take.page]} note?`,
  };
}

/** Mockup notes (design-1110f), preview/test only and always tagged Sample data. Numbers match the mock fixtures. */
export const SAMPLE_NOTES: Record<TakePage, TakeNote> = {
  home: { text: "Recovery **74%** after **7h 32m** asleep: a good day for real work. **2** business decisions are waiting.", chips: [{ label: "Recovery", value: "74%" }, { label: "Sleep", value: "7h 32m" }, { label: "Decisions", value: "2" }], time: "Today, 7:40", sourceLine: "From Fitness and Business", question: "What should I start with today?" },
  fitness: { text: "Recovery is high at **74%** after **7h 32m** asleep at **91%** efficiency. Effort so far is **35 of 100**, so there's room for a harder session today.", chips: [{ label: "Recovery", value: "74%" }, { label: "Sleep", value: "7h 32m" }, { label: "Efficiency", value: "91%" }, { label: "Effort", value: "35 / 100" }], time: "Today, 7:40", sourceLine: "From last night's sleep and today's effort", question: "Why is there room for a harder session today?" },
  neuro: { text: "Last night's **20-minute** session averaged **62 focus** and **71 calm**. You went in on **74%** recovery, and calm led focus, so a short focus block may suit you today.", chips: [{ label: "Focus", value: "62 / 100" }, { label: "Calm", value: "71 / 100" }, { label: "Session", value: "20 min" }, { label: "Recovery", value: "74%" }], time: "Today, 7:40", sourceLine: "From the Oct 27 session", question: "What kind of focus block would suit me today?" },
  mindBody: { lane: "Health + Neuro", text: "Body and mind mostly agree today. Resting HR is **50 bpm**, 2 below your usual, and HRV is **56 ms**, a little under your 60. Last night's session held calm at **71** and focus at **62**. Favour steady work over a hard workout.", chips: [{ label: "Resting HR", value: "50 bpm", lane: "health" }, { label: "HRV", value: "56 ms", lane: "health" }, { label: "Sleep", value: "7h 32m", lane: "health" }, { label: "Calm", value: "71", lane: "neuro" }, { label: "Focus", value: "62", lane: "neuro" }], time: "Today, 7:40", sourceLine: "From today's readiness and the Oct 27 session", question: "Why steady work over a hard workout today?" },
  business: { text: "Net is **$1,580** over 30 days on **$1,820** revenue. Two things need you: the Taiyo Skin funding vote and the supplier invoice with no tax line. Trading is within limits at risk index **0.18**.", chips: [{ label: "Net · 30 days", value: "$1,580" }, { label: "Revenue", value: "$1,820" }, { label: "Needs a decision", value: "2" }, { label: "Risk index", value: "0.18" }], time: "Today, 7:40", sourceLine: "From Enterprise overview", question: "Which decision should I take first?" },
  agents: { text: "Amara is online. Health checks ran this morning. Nothing has been delivered yet.", chips: [], time: "Today, 7:40", sourceLine: "From Agents", question: "What ran today?" },
  smarthome: { text: "Household controls are grouped by type. Backup sensors stay out of those groups.", chips: [], time: "Today, 7:40", sourceLine: "From Smart home", question: "What's on right now?" },
  life: { text: "This week's habits are **4 of 5** on track, and the lease notice is due by **Mar 1**. Sunday's weekly review is the place to decide whether to keep the reading habit.", chips: [{ label: "Habits this week", value: "4 of 5" }, { label: "Lease notice", value: "Mar 1" }, { label: "Weekly review", value: "Sunday" }], time: "Today, 7:40", sourceLine: "From Life · Goals and Dates", question: "What should I look at first in Life today?" },
};

/** Preview only: `nexus.sampleTakeState` shows one of the other states with sample adapters on. */
export const SAMPLE_TAKE_STATE_KEY = "nexus.sampleTakeState";
function sampleState(): string | null {
  try { return globalThis.localStorage?.getItem(SAMPLE_TAKE_STATE_KEY) ?? null; } catch { return null; }
}

export function sampleTakeView(page: TakePage, forced: string | null = sampleState()): TakeView {
  switch (forced) {
    case "generating": return { state: "generating" };
    case "nodata": return { state: "nodata", waitingFor: WAITING_FOR[page], sample: true };
    case "failed": return { state: "failed", reason: "Couldn't write today's note" };
    case "unwired": return { state: "unwired" };
    default: return { state: "fresh", note: SAMPLE_NOTES[page], sample: true };
  }
}

/** The hook result as a card state. A live take is "fresh"; nothing yet is "generating" while the first check runs, otherwise unwired. */
export function takeView(page: TakePage, result: TakeResult, checking: boolean): TakeView {
  if (result.state === "live") return { state: "fresh", note: liveNote(result.value), sample: false };
  if (result.state === "sample") return { state: "fresh", note: liveNote(result.value), sample: true };
  if (checking) return { state: "generating" };
  return { state: "unwired" };
}

/** In preview mode the live generator is never asked (no router call for a sample screen). */
export const PREVIEW_DEPS: TakeDeps = { now: () => Date.now(), kv: { load: async () => null, save: async () => undefined }, ask: async () => ({ content: "", tier: "none" }), facts: async () => null };
export const previewMode = sampleAdaptersEnabled;

/** "Ask more ›": Home chat opens with this context strip and the question pre-filled; nothing is sent. */
export const ASK_MORE_EVENT = "amara:ask-more";
export type AskMoreDetail = { from: string; chips: TakeChip[]; question: string; sample: boolean };
export function askMore(detail: AskMoreDetail) {
  window.dispatchEvent(new CustomEvent<AskMoreDetail>(ASK_MORE_EVENT, { detail }));
}
