import { invoke } from "@tauri-apps/api/core";
import { activityBus } from "./activityBus";

/**
 * Nightly goal-roadmap drafting (decision 0013). Once per night, local time (Taisun's Mac is on
 * JST), between 02:00 and 05:00, when nothing else is running on the activity bus, call
 * `roadmap_draft_pending`. Overdue after a sleeping Mac: it runs at the next tick inside the window
 * (the window is the "next opportunity"; outside it the job waits for the next night).
 * The deep call and the Claude gate live in Rust; this file only decides when.
 */

export const ROADMAP_NIGHT_START_HOUR = 2;
export const ROADMAP_NIGHT_END_HOUR = 5; // exclusive
export const ROADMAP_NIGHT_KEY = "nexus.roadmapNight.lastDay";

export const localDayKey = (now: number): string => {
  const d = new Date(now);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
};

export const inRoadmapWindow = (now: number): boolean => {
  const h = new Date(now).getHours();
  return h >= ROADMAP_NIGHT_START_HOUR && h < ROADMAP_NIGHT_END_HOUR;
};

/** Pure decision. `busy` = some other task is running; `lastDay` = the last night it ran (YYYY-MM-DD). */
export function roadmapNightDue(now: number, lastDay: string | null, busy: boolean): boolean {
  return inRoadmapWindow(now) && !busy && lastDay !== localDayKey(now);
}

const readLast = (): string | null => {
  try { return globalThis.localStorage?.getItem(ROADMAP_NIGHT_KEY) ?? null; } catch { return null; }
};
const writeLast = (day: string): void => {
  try { globalThis.localStorage?.setItem(ROADMAP_NIGHT_KEY, day); } catch { /* it just may run again, which is harmless */ }
};

/** Called from the scheduler tick. Never throws. A failed run is retried at the next tick in the window. */
export async function tickRoadmapNight(now: number = Date.now()): Promise<void> {
  let busy = true;
  try { busy = activityBus.getActiveTasks().length > 0; } catch { return; }
  if (!roadmapNightDue(now, readLast(), busy)) return;
  const id = activityBus.register("Goal roadmaps", "scheduler");
  try {
    const n = await invoke<number>("roadmap_draft_pending");
    writeLast(localDayKey(now));
    activityBus.complete(id, n > 0 ? `${n} roadmap update(s) drafted` : "Nothing to draft");
  } catch {
    activityBus.fail(id, "Roadmap drafting failed; retrying next tick");
  }
}
