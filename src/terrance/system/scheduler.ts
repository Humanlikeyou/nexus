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
const DAILY_SUMMARY_COOLDOWN_MS = 20 * 60 * 60 * 1000;   // 20 hours (prevents double-send)
const MIDDAY_COOLDOWN_MS = 20 * 60 * 60 * 1000;           // 20 hours (prevents double-send)
const EVENING_COOLDOWN_MS = 20 * 60 * 60 * 1000;          // 20 hours (prevents double-send)
const PRUNE_INTERVAL_MS = 7 * 24 * 60 * 60 * 1000;       // 7 days
const CHECK_INTERVAL_MS = 5 * 60 * 1000;                  // Check every 5 minutes
const DEFAULT_SUMMARY_HOUR = 7;                            // 7 AM local time
const DEFAULT_MIDDAY_HOUR = 12;                            // 12:30 PM local time
const DEFAULT_EVENING_HOUR = 21;                           // 9 PM local time
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

/** Window key for a job; failure counters reset when it changes. */
function windowKeyFor(id: JobId, now: number): string {
  if (id === "daily" || id === "midday" || id === "evening") return new Date(now).toDateString();
  return "interval";
}

function inBackoff(id: JobId, now: number): boolean {
  const f = failures.get(id);
  if (!f) return false;
  if (f.window !== windowKeyFor(id, now)) {
    failures.delete(id);
    return false;
  }
  return f.count >= MAX_ATTEMPTS_PER_WINDOW || now < f.nextAt;
}

function recordFailure(id: JobId, now: number): void {
  const window = windowKeyFor(id, now);
  const prev = failures.get(id);
  const count = prev && prev.window === window ? prev.count + 1 : 1;
  const delay = BACKOFF_DELAYS_MS[Math.min(count - 1, BACKOFF_DELAYS_MS.length - 1)];
  failures.set(id, { count, nextAt: now + delay, window });
}

function recordSuccess(id: JobId): void {
  failures.delete(id);
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
  detail?: string,
): Promise<void> {
  const taskId = activityBus.register(label, kind, detail ? { detail } : undefined);
  console.log(`[scheduler] Running ${label}${detail ? ` (${detail})` : ""}...`);
  let outcome: JobOutcome;
  try {
    outcome = await fn();
  } catch (e) {
    outcome = { ok: false, detail: String(e) };
  }
  if (outcome.ok) {
    recordSuccess(id);
    activityBus.complete(taskId, outcome.detail);
  } else {
    recordFailure(id, now);
    activityBus.fail(taskId, outcome.detail);
    console.error(`[scheduler] ${label} failed: ${outcome.detail}`);
  }
}

async function tick() {
  if (running) return;
  running = true;

  try {
    let ctx: TerranceContext;
    try {
      ctx = await loadContext();
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
        await patchContext(seed);
      } catch (e) {
        console.error("[scheduler] could not persist seeded timestamps:", e);
      }
    }
    const currentHour = new Date(now).getHours();
    const summaryHour = ctx.dailySummaryHour ?? DEFAULT_SUMMARY_HOUR;

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

    const heavy = (id: JobId) => heavyBudget > 0 && !inBackoff(id, now);
    const sentOutcome = (r: { sent: boolean }): JobOutcome =>
      r.sent ? { ok: true, detail: "Sent via Telegram" } : { ok: false, detail: "Generation or delivery failed" };

    // Daily summary — runs at the configured hour, or catches up if missed
    const lastSummary = ctx.lastDailySummaryTimestamp ?? 0;
    const missedSummary = now - lastSummary > DAILY_SUMMARY_COOLDOWN_MS;
    const isScheduledHour = currentHour === summaryHour;
    const isCatchUp = missedSummary && currentHour >= summaryHour;
    if (missedSummary && (isScheduledHour || isCatchUp) && heavy("daily")) {
      heavyBudget--;
      await runJob("daily", "Daily Summary", "daily_summary",
        async () => sentOutcome(await generateDailySummary()), now,
        isCatchUp && !isScheduledHour ? "catch-up" : undefined);
    }

    // Midday correction
    const lastMidday = ctx.lastMiddayTimestamp ?? 0;
    const missedMidday = now - lastMidday > MIDDAY_COOLDOWN_MS;
    const isMiddayHour = currentHour === DEFAULT_MIDDAY_HOUR;
    const isMiddayCatchUp = missedMidday && currentHour > DEFAULT_MIDDAY_HOUR && currentHour < DEFAULT_EVENING_HOUR;
    if (missedMidday && (isMiddayHour || isMiddayCatchUp) && heavy("midday")) {
      heavyBudget--;
      await runJob("midday", "Midday Correction", "daily_summary",
        async () => sentOutcome(await generateMiddayCorrection()), now,
        isMiddayCatchUp && !isMiddayHour ? "catch-up" : undefined);
    }

    // Evening integration
    const lastEvening = ctx.lastEveningTimestamp ?? 0;
    const missedEvening = now - lastEvening > EVENING_COOLDOWN_MS;
    const isEveningHour = currentHour === DEFAULT_EVENING_HOUR;
    const isEveningCatchUp = missedEvening && currentHour > DEFAULT_EVENING_HOUR;
    if (missedEvening && (isEveningHour || isEveningCatchUp) && heavy("evening")) {
      heavyBudget--;
      await runJob("evening", "Evening Integration", "daily_summary",
        async () => sentOutcome(await generateEveningIntegration()), now,
        isEveningCatchUp && !isEveningHour ? "catch-up" : undefined);
    }

    // Ritual reminders (presence drills, training, weekly/monthly check-ins)
    try {
      await tickRituals();
    } catch (e) {
      console.error("[scheduler] Rituals tick error:", e);
    }

    // Health check (light, not budgeted)
    const lastHealthCheck = ctx.lastHealthCheckTimestamp ?? 0;
    if (now - lastHealthCheck >= HEALTH_CHECK_INTERVAL_MS && !inBackoff("health", now)) {
      await runJob("health", "Health Check", "health_check", async () => {
        const report = await runDiagnostics();
        if (report.issues.length > 0) {
          await autoHeal(report);
          return { ok: true, detail: `${report.issues.length} issue(s) addressed` };
        }
        return { ok: true, detail: "All systems healthy" };
      }, now);
    }

    // Consolidation (+ skills evolution on the same cadence)
    const lastConsolidation = ctx.lastConsolidationTimestamp ?? 0;
    if (now - lastConsolidation >= CONSOLIDATION_INTERVAL_MS && heavy("consolidation")) {
      heavyBudget--;
      await runJob("consolidation", "Memory Consolidation", "consolidation", async () => {
        const result = await consolidateRecentSessions();
        return { ok: true, detail: result.consolidated ? "Consolidated recent sessions" : "Skipped (not enough sessions)" };
      }, now);

      try {
        const evoId = activityBus.register("Skills Evolution", "skills_evolution");
        const evo = await evolveSkills();
        activityBus.complete(evoId, evo.created > 0 || evo.updated > 0
          ? `${evo.created} created, ${evo.updated} updated`
          : "No new patterns");
      } catch (e) {
        console.error("[scheduler] Skills evolution error:", e);
      }
    }

    // Reasoning
    const lastReasoning = ctx.lastReasoningTimestamp ?? 0;
    if (now - lastReasoning >= REASONING_INTERVAL_MS && heavy("reasoning")) {
      heavyBudget--;
      await runJob("reasoning", "Reasoning Loop", "reasoning", async () => {
        const result = await runReasoningLoop();
        return { ok: true, detail: result.reasoned ? "Reflection complete" : "Skipped" };
      }, now);
    }

    // Weekly pruning
    const lastPrune = ctx.lastPruneTimestamp ?? 0;
    if (now - lastPrune >= PRUNE_INTERVAL_MS && heavy("prune")) {
      heavyBudget--;
      await runJob("prune", "Memory Pruning", "prune", async () => {
        const result = await pruneMemory();
        return { ok: true, detail: `${result.archived} archived, ${result.pruned} trimmed` };
      }, now);
    }

    // Custom user-defined jobs
    try {
      const jobResults = await tickCustomJobs();
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
      await tickSubagents();
    } catch (e) {
      console.error("[scheduler] Subagents error:", e);
    }

    // Prune old activity bus entries from memory
    activityBus.prune();
  } catch (e) {
    console.error("[scheduler] Error:", e);
  } finally {
    running = false;
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
