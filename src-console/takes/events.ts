import { invoke } from "@tauri-apps/api/core";
import type { TakePage } from "../amaraTake/types";
import type { Answer, AnswerState, SkipReason } from "./adapters";
import { notifyTakeLog } from "./takeLog";

/**
 * The event store (commands/take_events.rs): append-only facts about suggestions, answers and settings.
 * Everything the adapter shows (today's answer, the week, outcomes) is a pure reduction over these events,
 * so a reload gives the same answer and an Undo or a midnight is just another event or a derived state.
 */
export type TakeEvent = { id: string; kind: "today" | "answer" | "undo" | "setting" | "type_action" | "ledger_seen"; at: number; page?: TakePage | null; day?: string | null; data: Record<string, unknown> };

let cache: TakeEvent[] | null = null;
let attempted = false;
export const takeEvents = (): TakeEvent[] | null => cache;
export const setTakeEvents = (e: TakeEvent[] | null) => { cache = e; notifyTakeLog(); };
export const resetTakeEventsForTest = () => { cache = null; attempted = false; };

export async function refreshTakeEvents(): Promise<void> {
  try { const list = await invoke<TakeEvent[]>("take_events_list"); if (Array.isArray(list)) cache = list; } catch { /* stays unloaded: reads stay unwired */ }
  notifyTakeLog();
}
export function ensureTakeEvents(): void { if (cache === null && !attempted) { attempted = true; void refreshTakeEvents(); } }

/** Appends one event; false when the store refused it (the caller then reports unwired, never success). */
export async function appendTakeEvent(e: TakeEvent): Promise<boolean> {
  try { await invoke<boolean>("take_events_append", { event: e }); } catch { return false; }
  if (cache && !cache.some((x) => x.id === e.id)) cache = [...cache, e];
  notifyTakeLog();
  return true;
}

// ---------------- pure reducers ----------------
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);

export const timeLabel = (ms: number) => new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", timeZone: "Asia/Tokyo" });

export type AnswerView = { answer: Answer; blockedDetection: boolean; evidenceRef: string | null };

/**
 * One suggestion's answer. A tap always wins over a detection; Undo resets to waiting and blocks detection for
 * that suggestion (otherwise the detector would re-mark it at once). A suggestion still unanswered after its day
 * ended is `unknown`, never `skipped`: that is derived here, so no scheduler has to run at midnight.
 */
export function answerOf(events: readonly TakeEvent[], sid: string, day: string, todayDay: string): AnswerView {
  let cur: Answer | null = null; let ref: string | null = null; let blocked = false;
  for (const e of events) {
    if (e.data.sid !== sid) continue;
    if (e.kind === "undo") { cur = null; ref = null; blocked = true; continue; }
    if (e.kind !== "answer") continue;
    const source = e.data.source === "detected" ? "detected" : "tap";
    if (source === "detected" && (blocked || cur?.source === "tap")) continue;
    cur = { state: (str(e.data.state) as AnswerState) ?? "waiting", source, at: timeLabel(e.at), reason: (str(e.data.reason) as SkipReason | null) ?? null, ...(str(e.data.detail) ? { detail: str(e.data.detail) as string } : {}) };
    ref = str(e.data.evidenceRef);
  }
  if (!cur) return { answer: { state: day < todayDay ? "unknown" : "waiting" }, blockedDetection: blocked, evidenceRef: null };
  return { answer: cur, blockedDetection: blocked, evidenceRef: ref };
}

export const autoDoneOn = (events: readonly TakeEvent[]): boolean => {
  let on = true;
  for (const e of events) if (e.kind === "setting" && typeof e.data.autoDone === "boolean") on = e.data.autoDone;
  return on;
};

export type TodayRecord =
  | { page: TakePage; day: string; kind: "suggestion"; recId: string; why: string; whySource: "model" | "template"; facts: Array<{ label: string; value: string; note: string }> }
  | { page: TakePage; day: string; kind: "followup"; type: string; verdict: string };

export function todayRecords(events: readonly TakeEvent[]): TodayRecord[] {
  return events.filter((e) => e.kind === "today" && e.page && e.day).map((e) => ({ page: e.page as TakePage, day: e.day as string, ...e.data }) as TodayRecord);
}
export const sidOf = (page: TakePage, day: string, recId: string) => `${page}-${day}-${recId}`;

export type TypeAction = { type: string; action: "keep" | "keep_suggesting" | "stop"; at: number; day: string; note: string | null };
export function typeActions(events: readonly TakeEvent[]): Map<string, TypeAction> {
  const m = new Map<string, TypeAction>();
  for (const e of events) if (e.kind === "type_action") m.set(String(e.data.type), { type: String(e.data.type), action: e.data.action as TypeAction["action"], at: e.at, day: e.day ?? "", note: str(e.data.note) });
  return m;
}
