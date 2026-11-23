import { invokeBound } from "../../../bridge";
import { activityBus } from "./activityBus";

/**
 * Nightly goal-roadmap drafting (decision 0013). Once per night, local time (Taisun's Mac is on
 * JST), between 02:00 and 05:00, when nothing else is running on the activity bus, call
 * `roadmap_draft_pending`. Catch-up (decision 0018): when a night was missed (the Mac slept through
 * the window) and goals are waiting, it also runs at ANY time of day once the Mac is on AC power,
 * the activity bus is idle and nothing (chat turn, crew run, Muse session) was active for 10
 * minutes. Rust owns "already ran today", the AC check, the 10-minute clock and the deep call;
 * this file only decides when, from what `roadmap_catchup_status` reports.
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

/** What Rust reports (`roadmap_catchup_status`). */
export interface CatchUpStatus {
  due: boolean;      // a roadmap is waiting in "drafting", or a smaller-step request is pending
  ranToday: boolean; // Rust's once-a-day record: the single source of truth
  onAc: boolean;
  busy: boolean;     // chat turn, crew run or Muse session right now
  idleSecs: number;  // since the last of those
}

export const CATCH_UP_IDLE_SECS = 10 * 60;

/** Pure: may an out-of-window catch-up run start now? (The window itself is `roadmapNightDue`.) */
export function catchUpAllowed(s: CatchUpStatus, busBusy: boolean): boolean {
  return s.due && !s.ranToday && s.onAc && !s.busy && !busBusy && s.idleSecs >= CATCH_UP_IDLE_SECS;
}

/** Pure: the scheduler gives roadmaps a turn when other heavy jobs have the budget only if they were skipped last tick. */
export function roadmapTurn(heavyBudget: number, skippedTicks: number): boolean {
  return heavyBudget > 0 || skippedTicks >= 1;
}

const readLast = (): string | null => {
  try { return globalThis.localStorage?.getItem(ROADMAP_NIGHT_KEY) ?? null; } catch { return null; }
};
const writeLast = (day: string): void => {
  try { globalThis.localStorage?.setItem(ROADMAP_NIGHT_KEY, day); } catch { /* it just may run again, which is harmless */ }
};

/** What a refusal from `roadmap_draft_pending` means for the scheduler. */
export function roadmapOutcome(err: unknown): "retry" | "done" | "failed" {
  const m = typeof err === "string" ? err : err instanceof Error ? err.message : "";
  if (m === "busy" || m === "not_idle") return "retry"; // not_idle: Rust refused a catch-up (battery or recent activity) // a chat turn, crew run or other draft is active: next tick inside the window
  if (m === "already_ran") return "done"; // Rust already completed a run today
  return "failed";
}

/** Called from the scheduler tick. Never throws. A busy or failed run is retried at the next tick. */
export async function tickRoadmapNight(now: number = Date.now()): Promise<void> {
  let busy = true;
  try { busy = activityBus.getActiveTasks().length > 0; } catch { return; }
  if (busy) return;
  const last = readLast();
  const today = localDayKey(now);
  if (last === today) return; // cheap bail; Rust still refuses a second run
  let catchUp = false;
  if (!roadmapNightDue(now, last, busy)) {
    // Outside 02:00-05:00: only a catch-up, and only when Rust says it is safe.
    try {
      const st = await invokeBound<CatchUpStatus>("roadmap_catchup_status");
      if (!catchUpAllowed(st, busy)) return;
    } catch { return; }
    catchUp = true;
  }
  const id = activityBus.register("Goal roadmaps", "scheduler");
  try {
    const n = await invokeBound<number>("roadmap_draft_pending", { catchUp });
    writeLast(today);
    activityBus.complete(id, n > 0 ? `${n} roadmap update(s) drafted` : "Nothing to draft");
  } catch (e) {
    const o = roadmapOutcome(e);
    if (o === "done") {
      writeLast(today);
      activityBus.complete(id, "Already drafted today");
    } else if (o === "retry") {
      activityBus.complete(id, "Busy; will try again next tick"); // lastDay is not written
    } else {
      activityBus.fail(id, "Roadmap drafting failed; retrying next tick");
    }
  }
}
