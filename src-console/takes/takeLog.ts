import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { tokyoDay } from "../lib/mindBody";
import type { AmaraTake, CitedPoint, TakePage } from "../amaraTake/types";
import type { HistoryEntry, HistoryFilter, MemoryLine } from "./adapters";

/**
 * The TakeLog (docs/design/HANDOFF-1111t.md §2, §11 step 1). The log itself lives in Rust
 * (commands/take_log.rs): append-only JSONL, one writer lock, shared with the Home heart-rate take.
 * This module is the webview's read cache plus pure selectors, so the synchronous `Wired<T>` adapter can
 * read it. Nothing here deletes by age: retention is computed when reading.
 */
export type TakeLogEntry = {
  id: string; page: TakePage; day: string; generatedAt: number;
  kind: "daily" | "followup"; text: string; cited: CitedPoint[]; fingerprint: string; tier: string;
  trialId: string | null; memoryLine: string | null; forgottenAt: number | null;
};

export const RETENTION_DAYS = 365;
const DAY_MS = 86_400_000;

/** TODO(1111t §11 step 1, Taisun 11 Nov): after 365 days a take is summarized into Memory. That step is NOT
 *  built and must not write into live Letta. Until then old takes are only hidden here (computed, not deleted). */
export const withinRetention = (entries: readonly TakeLogEntry[], now: number): TakeLogEntry[] =>
  entries.filter((e) => now - e.generatedAt < RETENTION_DAYS * DAY_MS);

/** The log id for a take. `<page>-<generatedAt>` is also what the Rust migration uses, so a migrated take and a
 *  later append of the same take never duplicate. */
export const takeEntryId = (page: TakePage, generatedAt: number) => `${page}-${generatedAt}`;

export function entryFromTake(take: AmaraTake, tier: string): TakeLogEntry {
  return { id: takeEntryId(take.page, take.generatedAt), page: take.page, day: take.day, generatedAt: take.generatedAt, kind: "daily", text: take.text, cited: take.cited, fingerprint: take.fingerprint, tier, trialId: null, memoryLine: null, forgottenAt: null };
}

const dayLabel = (day: string) => new Date(`${day}T00:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const weekdayLabel = (day: string) => new Date(`${day}T00:00:00Z`).toLocaleDateString("en-US", { weekday: "short", timeZone: "UTC" });
const mondayOf = (day: string) => { const d = new Date(`${day}T00:00:00Z`); return d.getTime() - ((d.getUTCDay() + 6) % 7) * DAY_MS; };
function weekLabel(day: string, today: string): string {
  const weeks = Math.round((mondayOf(today) - mondayOf(day)) / (7 * DAY_MS));
  return weeks <= 0 ? "This week" : weeks === 1 ? "Last week" : `Week of ${dayLabel(new Date(mondayOf(day)).toISOString().slice(0, 10))}`;
}

const live = (e: TakeLogEntry) => e.forgottenAt == null;
const byNewest = (a: TakeLogEntry, b: TakeLogEntry) => b.generatedAt - a.generatedAt;

/** History for one page, newest first. Forgotten and expired takes are not listed. Follow-ups only match
 *  `kind: "followup"`; "With a suggestion" is empty until suggestions are wired (no entry has a trial). */
export function historyView(entries: readonly TakeLogEntry[], page: TakePage, filter: HistoryFilter, q: string, now: number): HistoryEntry[] {
  const needle = q.trim().toLowerCase();
  const today = tokyoDay(new Date(now).toISOString()) ?? "";
  return withinRetention(entries, now)
    .filter((e) => e.page === page && live(e))
    .filter((e) => filter === "all" || (filter === "followup" ? e.kind === "followup" : e.trialId != null))
    .filter((e) => !needle || e.text.toLowerCase().includes(needle))
    .sort(byNewest)
    .map((e) => ({ id: e.id, week: weekLabel(e.day, today), day: dayLabel(e.day), weekday: weekdayLabel(e.day), text: e.text, kind: e.kind, suggestion: null, verdict: null }));
}

/** The "↺" line: a template over the newest stored daily take from an earlier day, quoting its first sentence.
 *  Never model recall. Home heart-rate takes carry their own memory line and are skipped here. */
export function memoryLineView(entries: readonly TakeLogEntry[], page: TakePage, today: string, now: number): MemoryLine | null {
  const prev = withinRetention(entries, now).filter((e) => e.page === page && live(e) && e.day < today && e.tier !== "home_hr" && !e.id.startsWith("home-hr-")).sort(byNewest)[0];
  if (!prev) return null;
  const first = prev.text.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? prev.text;
  return { day: dayLabel(prev.day), text: `On ${dayLabel(prev.day)} I said: “${first.trim()}”` };
}

// ---- cache (loaded from Rust, never invented) ----
let cache: TakeLogEntry[] | null = null;
let version = 0;
let migrated = false;
const listeners = new Set<() => void>();
const notify = () => { version += 1; listeners.forEach((l) => l()); };

export function setTakeLogCache(entries: TakeLogEntry[] | null) { cache = entries; notify(); }
export const takeLogEntries = (): TakeLogEntry[] | null => cache;

/** Loads the log (and, once per launch, copies the old kv `amara_take_<page>` records in; kv is never touched). */
export function refreshTakeLog(): Promise<void> {
  const run = (async () => {
    try {
      if (!migrated) { migrated = true; await invoke<number>("take_log_migrate").catch(() => 0); }
      cache = await invoke<TakeLogEntry[]>("take_log_list");
    } catch { /* stays as it was: unwired until a read works */ }
    notify();
  })();
  return run;
}
/** Starts the first load once per launch; a failed load is not retried by reads (that would loop on re-render). */
let attempted = false;
export function ensureTakeLog(): void { if (cache === null && !attempted) { attempted = true; void refreshTakeLog(); } }
export const resetTakeLogForTest = () => { cache = null; attempted = false; migrated = false; };

export async function forgetTake(id: string): Promise<boolean> {
  try {
    const ok = await invoke<boolean>("take_log_forget", { id });
    await refreshTakeLog();
    return ok;
  } catch { return false; }
}

/** Appends one accepted take. A failure never blocks the take itself. */
export async function appendTake(entry: TakeLogEntry): Promise<void> {
  try { await invoke<boolean>("take_log_append", { entry }); if (cache !== null) await refreshTakeLog(); } catch { /* the daily kv cache still holds it */ }
}

const subscribe = (l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; };
/** Re-renders the caller when the log loads or changes (the adapter itself is synchronous). */
export function useTakeLogVersion(): number { return useSyncExternalStore(subscribe, () => version, () => version); }
