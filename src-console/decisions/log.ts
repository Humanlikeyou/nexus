import type { Route } from "../app/routes";
import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import { UNDO_MS } from "../adds/flow";
import { announceStores, inTauri, live, realInvoke, type Invoke } from "../lib/localStores";

/**
 * design-1111v §4: the decisions log, separate from the business ledger. Append-only; Undo within 8 s deletes
 * the record, after that a correction is a new record. Recording a decision never moves money and never
 * contacts anyone: there is no method here that does either. Unwired until the Lead's store exists (§7.1).
 */
export type DecisionChoice = "yes" | "no" | "handled";
export type DecisionRecord = {
  id: string;
  /** The waiting item it answers ("f1" funding request, "t1" tax flag). */
  itemId: string;
  subject: "vote" | "tax_flag";
  title: string;
  choice: DecisionChoice;
  note?: string;
  /** ISO time; shown in JST. */
  decidedAt: string;
  /** Crew or venture that proposed it. */
  proposer: string;
  sourceLink: Route;
  sourceLabel: string;
  outcome?: string;
  outcomeAt?: string;
};
export type DecisionDraft = Omit<DecisionRecord, "id" | "decidedAt" | "outcome" | "outcomeAt">;

export interface DecisionsLog {
  wired: boolean;
  list(): Wired<DecisionRecord[]>;
  append(draft: DecisionDraft, at?: Date): Promise<Wired<DecisionRecord>>;
  /** Only within 8 s of decidedAt. */
  undo(id: string, at?: Date): Promise<Wired<{ ok: boolean }>>;
  addOutcome(id: string, outcome: string, at?: Date): Promise<Wired<DecisionRecord>>;
}

/** Fired after a record is appended or undone, so Needs you today and the venture page re-read the log. */
export const DECISIONS_EVENT = "amara:decisions";
export const announceDecisions = () => { try { window.dispatchEvent(new Event(DECISIONS_EVENT)); } catch { /* no window in node tests */ } };

export const unwiredDecisions: DecisionsLog = { wired: false, list: () => UNWIRED, append: async () => UNWIRED, undo: async () => UNWIRED, addOutcome: async () => UNWIRED };

/** "Oct 28 · 14:02" in JST. */
export function jst(iso: string): string {
  const d = new Date(iso);
  const day = d.toLocaleDateString("en-US", { timeZone: "Asia/Tokyo", month: "short", day: "numeric" });
  const t = d.toLocaleTimeString("en-GB", { timeZone: "Asia/Tokyo", hour: "2-digit", minute: "2-digit" });
  return `${day} · ${t}`;
}
/** The Enterprise recommendation "Decide before noon" counts itself when a record lands before 12:00 JST. */
export function beforeNoonJst(iso: string): boolean {
  return Number(new Date(iso).toLocaleString("en-GB", { timeZone: "Asia/Tokyo", hour: "2-digit", hour12: false })) < 12;
}

const SEED: DecisionRecord[] = [
  { id: "d3", itemId: "f0", subject: "vote", title: "Taiyo Skin · second supplier quote", choice: "yes", decidedAt: "2026-10-09T16:05:00+09:00", proposer: "Taiyo Skin crew", sourceLink: { id: "enterprise", view: "ventures/taiyo-skin" }, sourceLabel: "Ventures › Taiyo Skin ›" },
  { id: "d2", itemId: "r1", subject: "vote", title: "Trading · raise daily loss cap to $400", choice: "no", decidedAt: "2026-10-14T09:40:00+09:00", proposer: "Trading strategy", sourceLink: { id: "enterprise", view: "ventures" }, sourceLabel: "Ventures › Trading ›", outcome: "Cap stayed at $250", outcomeAt: "2026-10-15T09:00:00+09:00" },
];

/** Mock mode only. In-memory, so nothing persists and nothing leaves the page. */
export function makeSampleDecisions(): DecisionsLog {
  let records = [...SEED].sort((a, b) => b.decidedAt.localeCompare(a.decidedAt));
  let n = 0;
  return {
    wired: true,
    list: () => sample([...records]),
    append: async (draft, at = new Date()) => {
      const r: DecisionRecord = { ...draft, id: `dn${++n}`, decidedAt: at.toISOString() };
      records = [r, ...records];
      return sample(r);
    },
    undo: async (id, at = new Date()) => {
      const r = records.find((x) => x.id === id);
      if (!r || at.getTime() - Date.parse(r.decidedAt) > UNDO_MS) return sample({ ok: false });
      records = records.filter((x) => x.id !== id);
      return sample({ ok: true });
    },
    addOutcome: async (id, outcome, at = new Date()) => {
      const r = records.find((x) => x.id === id);
      if (!r) return UNWIRED;
      // Append-only: the outcome fills the record's empty outcome fields; it never rewrites the choice.
      const next = { ...r, outcome, outcomeAt: at.toISOString() };
      records = records.map((x) => (x.id === id ? next : x));
      return sample(next);
    },
  };
}

/**
 * The real log (src-tauri decisions_log.rs). It calls only decisions_* commands: there is no path to the business
 * ledger or to anyone outside. wired flips to true after the first successful read; until then (or when the
 * backend is missing) it reads as unwired. A failed write resolves unwired and records nothing.
 */
export function makeLiveDecisions(call: Invoke = realInvoke): DecisionsLog & { refresh(): Promise<void> } {
  let cache: DecisionRecord[] = [];
  let ok = false;
  const refresh = async () => {
    try { cache = await call<DecisionRecord[]>("decisions_list"); ok = true; } catch { ok = false; }
    announceStores();
  };
  return {
    get wired() { return ok; },
    refresh,
    list: () => (ok ? live([...cache]) : UNWIRED),
    append: async (draft) => {
      try { const r = await call<DecisionRecord>("decisions_append", { draft }); cache = [r, ...cache]; return live(r); } catch { return UNWIRED; }
    },
    undo: async (id) => {
      try { const done = await call<boolean>("decisions_undo", { id }); if (done) cache = cache.filter((x) => x.id !== id); return live({ ok: done }); } catch { return UNWIRED; }
    },
    addOutcome: async (id, outcome) => {
      try { const r = await call<DecisionRecord>("decisions_add_outcome", { id, outcome }); cache = cache.map((x) => (x.id === id ? r : x)); return live(r); } catch { return UNWIRED; }
    },
  };
}

let sampleLog: DecisionsLog | null = null;
let liveLog: ReturnType<typeof makeLiveDecisions> | null = null;
export const decisionsLog = (): DecisionsLog => {
  if (sampleAdaptersEnabled()) return (sampleLog ??= makeSampleDecisions());
  if (!inTauri()) return unwiredDecisions;
  if (!liveLog) { liveLog = makeLiveDecisions(); void liveLog.refresh(); }
  return liveLog;
};
/** Test seam. */
export const resetSampleDecisions = () => { sampleLog = null; liveLog = null; };
