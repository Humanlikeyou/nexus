/**
 * Terrance Scheduler
 *
 * Manages periodic background tasks:
 *  - Memory consolidation (every 6 hours)
 *  - Autonomous reasoning loop (every 12 hours)
 *  - Daily summary / morning briefing (once per day at configured hour)
 *  - Health check + auto-healing (every 30 minutes)
 *  - Memory pruning (weekly)
 *
 * Uses timestamps persisted in terrance_context.json to avoid
 * duplicate runs after app restart.
 */

import {
  loadContext,
  patchContext,
  type TerranceContext,
  consolidateRecentSessions,
  runReasoningLoop,
  generateDailySummary,
  generateMiddayCorrection,
  generateEveningIntegration,
  pruneMemory,
} from "./continuity";
import { runDiagnostics, autoHeal } from "./selfDiagnosis";
import { tickRituals } from "./rituals";
import { tickCustomJobs } from "./customJobs";
import { evolveSkills } from "./skillsEvolution";
import { activityBus } from "./activityBus";
import { tickSubagents } from "./subagents";
import { startTelegramBridge, stopTelegramBridge } from "./telegramBridge";

const CONSOLIDATION_INTERVAL_MS = 6 * 60 * 60 * 1000;  // 6 hours
const REASONING_INTERVAL_MS = 12 * 60 * 60 * 1000;      // 12 hours
const HEALTH_CHECK_INTERVAL_MS = 30 * 60 * 1000;         // 30 minutes
const PRUNE_INTERVAL_MS = 7 * 24 * 60 * 60 * 1000;       // 7 days
const CHECK_INTERVAL_MS = 5 * 60 * 1000;                  // Check every 5 minutes
const DEFAULT_SUMMARY_HOUR = 7;                            // 7 AM local time
const DEFAULT_MIDDAY_HOUR = 12;
const DEFAULT_MIDDAY_MINUTE = 30;                          // 12:30 PM local time
const DEFAULT_EVENING_HOUR = 21;                           // 9 PM local time
const HOUR_MS = 60 * 60 * 1000;
/** Daily-summary catch-up ends at the midday slot (12:30). */
const DAILY_CATCHUP_UNTIL_MIN = DEFAULT_MIDDAY_HOUR * 60 + DEFAULT_MIDDAY_MINUTE;
/** Midday catch-up ends at the evening slot (21:00). */
const MIDDAY_WINDOW_MS = (DEFAULT_EVENING_HOUR * 60 - DAILY_CATCHUP_UNTIL_MIN) * 60_000;
/** Evening catch-up runs until 03:00 the next day. */
const EVENING_CATCHUP_UNTIL_HOUR = 3;
const EVENING_WINDOW_MS = (24 - DEFAULT_EVENING_HOUR + EVENING_CATCHUP_UNTIL_HOUR) * HOUR_MS;
const MAX_HEAVY_JOBS_PER_TICK = 1;                         // LLM / Telegram jobs per tick

/** Timestamps whose absence means "first observation": seed to now instead of firing. */
const SEEDED_TIMESTAMPS = [
  "lastDailySummaryTimestamp",
  "lastMiddayTimestamp",
  "lastEveningTimestamp",
  "lastConsolidationTimestamp",
  "lastReasoningTimestamp",
  "lastPruneTimestamp",
] as const;

let checkIntervalId: ReturnType<typeof setInterval> | null = null;
let startupTimerId: ReturnType<typeof setTimeout> | null = null;
let running = false;

export interface SchedulerStatus {
  running: boolean;
  lastConsolidation: number | null;
  lastReasoning: number | null;
  lastDailySummary: number | null;
  lastHealthCheck: number | null;
  lastPrune: number | null;
  dailySummaryHour: number;
}

let _lastStatus: SchedulerStatus = {
  running: false,
  lastConsolidation: null,
  lastReasoning: null,
  lastDailySummary: null,
  lastHealthCheck: null,
  lastPrune: null,
  dailySummaryHour: DEFAULT_SUMMARY_HOUR,
};

export function getSchedulerStatus(): SchedulerStatus {
  return { ..._lastStatus };
}

// ─── Slot-based timing (bug 5) ─────────────────────────────────────────────────

/** Most recent local occurrence of hh:mm at or before `now` (ms). */
export function lastSlot(now: number, hour: number, minute = 0): number {
  const s = new Date(now);
  s.setHours(hour, minute, 0, 0);
  if (s.getTime() > now) s.setDate(s.getDate() - 1);
  return s.getTime();
}

/**
 * A daily touch is due when it has not run since its latest slot and we are
 * still inside the catch-up window after that slot. A missing timestamp is
 * not due (the scheduler seeds it instead).
 */
export function isTouchDue(
  last: number | undefined,
  now: number,
  hour: number,
  minute: number,
  windowMs: number,
): { due: boolean; catchUp: boolean; slot: number } {
  const slot = lastSlot(now, hour, minute);
  const due = last != null && last > 0 && last < slot && now - slot < windowMs;
  return { due, catchUp: due && now - slot > CHECK_INTERVAL_MS * 2, slot };
}

export function clampSummaryHour(h: unknown): number {
  return typeof h === "number" && Number.isInteger(h) && h >= 0 && h <= 23 ? h : DEFAULT_SUMMARY_HOUR;
}

export type SchedulerTimestamps = Pick<
  TerranceContext,
  | "dailySummaryHour"
  | "lastDailySummaryTimestamp"
  | "lastMiddayTimestamp"
  | "lastEveningTimestamp"
  | "lastHealthCheckTimestamp"
  | "lastConsolidationTimestamp"
  | "lastReasoningTimestamp"
  | "lastPruneTimestamp"
>;

export interface DueTask {
  id: JobId;
  /** True when running late inside the catch-up window. */
  catchUp: boolean;
  /** Backoff window key: the slot start for daily touches, "interval" otherwise. */
  window: string;
}

/**
 * Pure decision: which jobs are due at `now` (ms, local time), in run order.
 * Does not apply failure backoff or the one-heavy-job-per-tick budget; tick()
 * does that.
 */
export function decideDueTasks(ctx: SchedulerTimestamps, now: number): DueTask[] {
  const due: DueTask[] = [];
  const summaryHour = clampSummaryHour(ctx.dailySummaryHour);
  const dailyWindowMs = Math.max((DAILY_CATCHUP_UNTIL_MIN - summaryHour * 60) * 60_000, HOUR_MS);

  const touches: [JobId, number | undefined, number, number, number][] = [
    ["daily", ctx.lastDailySummaryTimestamp, summaryHour, 0, dailyWindowMs],
    ["midday", ctx.lastMiddayTimestamp, DEFAULT_MIDDAY_HOUR, DEFAULT_MIDDAY_MINUTE, MIDDAY_WINDOW_MS],
    ["evening", ctx.lastEveningTimestamp, DEFAULT_EVENING_HOUR, 0, EVENING_WINDOW_MS],
  ];
  for (const [id, last, h, m, w] of touches) {
    const t = isTouchDue(last, now, h, m, w);
    if (t.due) due.push({ id, catchUp: t.catchUp, window: String(t.slot) });
  }

  const intervals: [JobId, number | undefined, number, boolean][] = [
    // Health check may run with a missing timestamp (light, not seeded).
    ["health", ctx.lastHealthCheckTimestamp ?? 0, HEALTH_CHECK_INTERVAL_MS, true],
    ["consolidation", ctx.lastConsolidationTimestamp, CONSOLIDATION_INTERVAL_MS, false],
    ["reasoning", ctx.lastReasoningTimestamp, REASONING_INTERVAL_MS, false],
    ["prune", ctx.lastPruneTimestamp, PRUNE_INTERVAL_MS, false],
  ];
  for (const [id, last, interval, allowMissing] of intervals) {
    if (last == null || (!allowMissing && last <= 0)) continue;
    if (now - last >= interval) due.push({ id, catchUp: false, window: "interval" });
  }
  return due;
}

// ─── Failure backoff (bug 3) ──────────────────────────────────────────────────
// Attempts are tracked in memory only (a restart may retry once; success still
// persists the real timestamp). Lead decision: retry after 5, 15, 60 min, max 3
// tries per window. No fixed 5-minute retry loop.

export type JobId = "daily" | "midday" | "evening" | "consolidation" | "reasoning" | "prune" | "health";

export const BACKOFF_DELAYS_MS = [5 * 60_000, 15 * 60_000, 60 * 60_000] as const;
export const MAX_ATTEMPTS_PER_WINDOW = 3;

interface FailureState {
  count: number;
  nextAt: number;
  window: string;
}
const failures = new Map<JobId, FailureState>();

/** Failure counters reset when the job's window (slot) changes. */
function inBackoff(id: JobId, now: number, window: string): boolean {
  const f = failures.get(id);
  if (!f) return false;
  if (f.window !== window) {
    failures.delete(id);
    return false;
  }
  return f.count >= MAX_ATTEMPTS_PER_WINDOW || now < f.nextAt;
}

function recordFailure(id: JobId, now: number, window: string): void {
  const prev = failures.get(id);
  const count = prev && prev.window === window ? prev.count + 1 : 1;
  const delay = BACKOFF_DELAYS_MS[Math.min(count - 1, BACKOFF_DELAYS_MS.length - 1)];
  failures.set(id, { count, nextAt: now + delay, window });
}

function recordSuccess(id: JobId): void {
  failures.delete(id);
}

// ─── Timeouts (bug 4) ─────────────────────────────────────────────────────────
// JS cannot cancel the underlying invoke; an orphaned promise may still finish
// and write its timestamp later, which is acceptable.

export const TASK_TIMEOUTS_MS: Record<JobId, number> = {
  daily: 3 * 60_000,
  midday: 3 * 60_000,
  evening: 3 * 60_000,
  consolidation: 5 * 60_000,
  reasoning: 5 * 60_000,
  prune: 2 * 60_000,
  health: 60_000,
};
const AUX_TIMEOUT_MS = 60_000;          // rituals, custom jobs, subagents, skills evolution
const LOAD_TIMEOUT_MS = 30_000;
export const TICK_WATCHDOG_MS = 15 * 60_000;
let tickStartedAt = 0;
let tickGen = 0;

export class TaskTimeoutError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TaskTimeoutError";
  }
}

export function withTimeout<T>(p: Promise<T>, ms: number, label: string): Promise<T> {
  let t: ReturnType<typeof setTimeout> | undefined;
  return Promise.race([
    p.finally(() => clearTimeout(t)),
    new Promise<never>((_, rej) => {
      t = setTimeout(() => rej(new TaskTimeoutError(`${label} timed out after ${ms}ms`)), ms);
    }),
  ]);
}

interface JobOutcome {
  ok: boolean;
  detail: string;
}

/** Run one job with its own try/catch so a throw never skips the rest of the tick. */
async function runJob(
  id: JobId,
  label: string,
  kind: Parameters<typeof activityBus.register>[1],
  fn: () => Promise<JobOutcome>,
  now: number,
  window: string,
  detail?: string,
): Promise<void> {
  const taskId = activityBus.register(label, kind, detail ? { detail } : undefined);
  console.log(`[scheduler] Running ${label}${detail ? ` (${detail})` : ""}...`);
  let outcome: JobOutcome;
  try {
    outcome = await withTimeout(fn(), TASK_TIMEOUTS_MS[id], label);
  } catch (e) {
    outcome = { ok: false, detail: String(e) };
  }
  if (outcome.ok) {
    recordSuccess(id);
    activityBus.complete(taskId, outcome.detail);
  } else {
    recordFailure(id, now, window);
    activityBus.fail(taskId, outcome.detail);
    console.error(`[scheduler] ${label} failed: ${outcome.detail}`);
  }
}

async function tick() {
  if (running) {
    if (Date.now() - tickStartedAt < TICK_WATCHDOG_MS) return;
    console.error("[scheduler] watchdog: previous tick stuck; forcing reset");
  }
  running = true;
  tickStartedAt = Date.now();
  const gen = ++tickGen;

  try {
    let ctx: TerranceContext;
    try {
      ctx = await withTimeout(loadContext(), LOAD_TIMEOUT_MS, "loadContext");
    } catch (e) {
      console.error("[scheduler] context unavailable; skipping tick (fail closed):", e);
      return; // finally still resets running
    }
    const now = Date.now();
    let heavyBudget = MAX_HEAVY_JOBS_PER_TICK;

    // Missing timestamps (fresh or partial context) are seeded, not treated as "infinitely overdue".
    const seed: Partial<TerranceContext> = {};
    for (const k of SEEDED_TIMESTAMPS) {
      if (!ctx[k]) seed[k] = now;
    }
    if (Object.keys(seed).length > 0) {
      ctx = { ...ctx, ...seed };
      try {
        await withTimeout(patchContext(seed), LOAD_TIMEOUT_MS, "seed timestamps");
      } catch (e) {
        console.error("[scheduler] could not persist seeded timestamps:", e);
      }
    }
    const summaryHour = clampSummaryHour(ctx.dailySummaryHour);

    // Update status cache
    _lastStatus = {
      running: true,
      lastConsolidation: ctx.lastConsolidationTimestamp ?? null,
      lastReasoning: ctx.lastReasoningTimestamp ?? null,
      lastDailySummary: ctx.lastDailySummaryTimestamp ?? null,
      lastHealthCheck: ctx.lastHealthCheckTimestamp ?? null,
      lastPrune: ctx.lastPruneTimestamp ?? null,
      dailySummaryHour: summaryHour,
    };

    const sentOutcome = (r: { sent: boolean }): JobOutcome =>
      r.sent ? { ok: true, detail: "Sent via Telegram" } : { ok: false, detail: "Generation or delivery failed" };

    const jobs: Record<JobId, { label: string; kind: Parameters<typeof activityBus.register>[1]; heavy: boolean; fn: () => Promise<JobOutcome> }> = {
      daily: { label: "Daily Summary", kind: "daily_summary", heavy: true, fn: async () => sentOutcome(await generateDailySummary()) },
      midday: { label: "Midday Correction", kind: "daily_summary", heavy: true, fn: async () => sentOutcome(await generateMiddayCorrection()) },
      evening: { label: "Evening Integration", kind: "daily_summary", heavy: true, fn: async () => sentOutcome(await generateEveningIntegration()) },
      health: {
        label: "Health Check", kind: "health_check", heavy: false,
        fn: async () => {
          const report = await runDiagnostics();
          if (report.issues.length > 0) {
            await autoHeal(report);
            return { ok: true, detail: `${report.issues.length} issue(s) addressed` };
          }
          return { ok: true, detail: "All systems healthy" };
        },
      },
      consolidation: {
        label: "Memory Consolidation", kind: "consolidation", heavy: true,
        fn: async () => {
          const result = await consolidateRecentSessions();
          return { ok: true, detail: result.consolidated ? "Consolidated recent sessions" : "Skipped (not enough sessions)" };
        },
      },
      reasoning: {
        label: "Reasoning Loop", kind: "reasoning", heavy: true,
        fn: async () => {
          const result = await runReasoningLoop();
          return { ok: true, detail: result.reasoned ? "Reflection complete" : "Skipped" };
        },
      },
      prune: {
        label: "Memory Pruning", kind: "prune", heavy: true,
        fn: async () => {
          const result = await pruneMemory();
          return { ok: true, detail: `${result.archived} archived, ${result.pruned} trimmed` };
        },
      },
    };

    // Ritual reminders (presence drills, training, weekly/monthly check-ins)
    try {
      await withTimeout(Promise.resolve(tickRituals()), AUX_TIMEOUT_MS, "rituals");
    } catch (e) {
      console.error("[scheduler] Rituals tick error:", e);
    }

    for (const task of decideDueTasks(ctx, now)) {
      const job = jobs[task.id];
      if (inBackoff(task.id, now, task.window)) continue;
      if (job.heavy) {
        if (heavyBudget <= 0) continue;
        heavyBudget--;
      }
      await runJob(task.id, job.label, job.kind, job.fn, now, task.window, task.catchUp ? "catch-up" : undefined);

      // Skills self-improvement runs on the consolidation cadence
      if (task.id === "consolidation") {
        try {
          const evoId = activityBus.register("Skills Evolution", "skills_evolution");
          const evo = await withTimeout(evolveSkills(), AUX_TIMEOUT_MS, "skills evolution");
          activityBus.complete(evoId, evo.created > 0 || evo.updated > 0
            ? `${evo.created} created, ${evo.updated} updated`
            : "No new patterns");
        } catch (e) {
          console.error("[scheduler] Skills evolution error:", e);
        }
      }
    }

    // Custom user-defined jobs
    try {
      const jobResults = await withTimeout(tickCustomJobs(), AUX_TIMEOUT_MS, "custom jobs");
      if (jobResults.length > 0) {
        const delivered = jobResults.filter((r) => r.delivered).length;
        const silent = jobResults.filter((r) => r.silent).length;
        console.log(`[scheduler] Custom jobs: ${jobResults.length} run, ${delivered} delivered, ${silent} silent.`);
      }
    } catch (e) {
      console.error("[scheduler] Custom jobs error:", e);
    }

    // Subagents
    try {
      await withTimeout(Promise.resolve(tickSubagents()), AUX_TIMEOUT_MS, "subagents");
    } catch (e) {
      console.error("[scheduler] Subagents error:", e);
    }

    // Prune old activity bus entries from memory
    activityBus.prune();
  } catch (e) {
    console.error("[scheduler] Error:", e);
  } finally {
    // A stale tick (reset by the watchdog) must not clear `running` for the new one.
    if (gen === tickGen) running = false;
  }
}

export function startScheduler(): void {
  if (checkIntervalId) return;
  console.log("[scheduler] Started (3-touch daily loop, consolidation=6h, reasoning=12h, health=30m, prune=weekly).");
  // Run first tick after a short delay to let the app fully initialize
  startupTimerId = setTimeout(tick, 30_000);
  checkIntervalId = setInterval(tick, CHECK_INTERVAL_MS);
  // Telegram bridge disabled — OpenClaw handles Telegram directly.
  // startTelegramBridge();
}

export function stopScheduler(): void {
  if (startupTimerId) { clearTimeout(startupTimerId); startupTimerId = null; }
  if (checkIntervalId) {
    clearInterval(checkIntervalId);
    checkIntervalId = null;
  }
  // stopTelegramBridge();
  _lastStatus.running = false;
  console.log("[scheduler] Stopped.");
}
