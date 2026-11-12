import { invoke } from "@tauri-apps/api/core";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import type { CrewsAdapter } from "./adapters";

/**
 * Live "Next run" for the Crews Schedules tile: the earliest real `nextRunAt` among enabled, active
 * automations (`list_terrence_automations`). No scheduled automation is an honest empty line, never a
 * made-up time. Until the first read, or if the read fails, it stays unwired.
 */
type Row = { enabled?: unknown; status?: unknown; nextRunAt?: unknown };
let cached: Wired<string> = UNWIRED;
let started = false;

const fmt = (d: Date) => d.toLocaleString("en-US", { weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

export function nextScheduleFrom(rows: unknown): Wired<string> {
  if (!Array.isArray(rows)) return UNWIRED;
  let best: Date | null = null;
  for (const r of rows as Row[]) {
    if (!r || r.enabled !== true || r.status !== "active" || typeof r.nextRunAt !== "string") continue;
    const d = new Date(r.nextRunAt);
    if (Number.isNaN(d.getTime())) continue;
    if (!best || d < best) best = d;
  }
  return { state: "live", value: best ? fmt(best) : "none scheduled" };
}

export async function refreshNextSchedule(): Promise<void> {
  try { cached = nextScheduleFrom(await invoke("list_terrence_automations", { limit: null })); } catch { cached = UNWIRED; }
}

export function makeLiveCrewsSchedule(base: CrewsAdapter): CrewsAdapter {
  if (!started) { started = true; void refreshNextSchedule(); }
  return { ...base, nextSchedule: () => cached };
}
export const resetLiveScheduleForTest = () => { cached = UNWIRED; started = false; };
