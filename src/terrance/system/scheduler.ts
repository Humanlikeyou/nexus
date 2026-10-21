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
    const currentHour = new Date().getHours();
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

    // Daily summary check — runs at the configured hour, or catches up if missed
    const lastSummary = ctx.lastDailySummaryTimestamp ?? 0;
    const missedSummary = now - lastSummary > DAILY_SUMMARY_COOLDOWN_MS;
    const isScheduledHour = currentHour === summaryHour;
    const isCatchUp = missedSummary && currentHour >= summaryHour;
    if (heavyBudget > 0 && missedSummary && (isScheduledHour || isCatchUp)) {
      heavyBudget--;
      const taskId = activityBus.register("Daily Summary", "daily_summary", {
        detail: isCatchUp && !isScheduledHour ? "catch-up" : undefined,
      });
      console.log(`[scheduler] Running daily summary${isCatchUp && !isScheduledHour ? " (catch-up)" : ""}...`);
      const result = await generateDailySummary();
      if (result.sent) {
        activityBus.complete(taskId, "Sent via Telegram");
        console.log("[scheduler] Daily summary sent via Telegram.");
      } else {
        activityBus.fail(taskId, "Generation failed");
        console.log("[scheduler] Daily summary generation failed.");
      }
    }

    // Midday correction check — runs around 12:30 PM
    const lastMidday = ctx.lastMiddayTimestamp ?? 0;
    const missedMidday = now - lastMidday > MIDDAY_COOLDOWN_MS;
    const isMiddayHour = currentHour === DEFAULT_MIDDAY_HOUR;
    const isMiddayCatchUp = missedMidday && currentHour > DEFAULT_MIDDAY_HOUR && currentHour < DEFAULT_EVENING_HOUR;
    if (heavyBudget > 0 && missedMidday && (isMiddayHour || isMiddayCatchUp)) {
      heavyBudget--;
      const taskId = activityBus.register("Midday Correction", "daily_summary", {
        detail: isMiddayCatchUp && !isMiddayHour ? "catch-up" : undefined,
      });
      console.log(`[scheduler] Running midday correction${isMiddayCatchUp && !isMiddayHour ? " (catch-up)" : ""}...`);
      const result = await generateMiddayCorrection();
      if (result.sent) {
        activityBus.complete(taskId, "Sent via Telegram");
        console.log("[scheduler] Midday correction sent via Telegram.");
      } else {
        activityBus.fail(taskId, "Generation failed");
      }
    }

    // Evening integration check — runs around 9 PM
    const lastEvening = ctx.lastEveningTimestamp ?? 0;
    const missedEvening = now - lastEvening > EVENING_COOLDOWN_MS;
    const isEveningHour = currentHour === DEFAULT_EVENING_HOUR;
    const isEveningCatchUp = missedEvening && currentHour > DEFAULT_EVENING_HOUR;
    if (heavyBudget > 0 && missedEvening && (isEveningHour || isEveningCatchUp)) {
      heavyBudget--;
      const taskId = activityBus.register("Evening Integration", "daily_summary", {
        detail: isEveningCatchUp && !isEveningHour ? "catch-up" : undefined,
      });
      console.log(`[scheduler] Running evening integration${isEveningCatchUp && !isEveningHour ? " (catch-up)" : ""}...`);
      const result = await generateEveningIntegration();
      if (result.sent) {
        activityBus.complete(taskId, "Sent via Telegram");
        console.log("[scheduler] Evening integration sent via Telegram.");
      } else {
        activityBus.fail(taskId, "Generation failed");
      }
    }

    // Ritual reminders (presence drills, training, weekly/monthly check-ins)
    try {
      await tickRituals();
    } catch (e) {
      console.error("[scheduler] Rituals tick error:", e);
    }

    // Health check
    const lastHealthCheck = ctx.lastHealthCheckTimestamp ?? 0;
    if (now - lastHealthCheck >= HEALTH_CHECK_INTERVAL_MS) {
      const taskId = activityBus.register("Health Check", "health_check");
      console.log("[scheduler] Running health check...");
      try {
        const report = await runDiagnostics();
        if (report.issues.length > 0) {
          activityBus.update(taskId, "running", `${report.issues.length} issue(s), auto-healing...`);
          console.log(`[scheduler] ${report.issues.length} issue(s) found, attempting auto-heal...`);
          await autoHeal(report);
          activityBus.complete(taskId, `${report.issues.length} issue(s) addressed`);
        } else {
          activityBus.complete(taskId, "All systems healthy");
          console.log("[scheduler] All systems healthy.");
        }
      } catch (e) {
        activityBus.fail(taskId, String(e));
        console.error("[scheduler] Health check error:", e);
      }
    }

    // Consolidation check
    const lastConsolidation = ctx.lastConsolidationTimestamp ?? 0;
    if (heavyBudget > 0 && now - lastConsolidation >= CONSOLIDATION_INTERVAL_MS) {
      heavyBudget--;
      const taskId = activityBus.register("Memory Consolidation", "consolidation");
      console.log("[scheduler] Running memory consolidation...");
      const result = await consolidateRecentSessions();
      if (result.consolidated) {
        activityBus.complete(taskId, "Consolidated recent sessions");
        console.log("[scheduler] Consolidation complete.");
      } else {
        activityBus.complete(taskId, "Skipped (not enough sessions)");
        console.log("[scheduler] Consolidation skipped (not enough sessions).");
      }

      // Skills self-improvement (runs on consolidation cadence)
      try {
        const evoId = activityBus.register("Skills Evolution", "skills_evolution");
        const evo = await evolveSkills();
        if (evo.created > 0 || evo.updated > 0) {
          activityBus.complete(evoId, `${evo.created} created, ${evo.updated} updated`);
          console.log(`[scheduler] Skills evolution: ${evo.created} created, ${evo.updated} updated.`);
        } else {
          activityBus.complete(evoId, "No new patterns");
        }
      } catch (e) {
        console.error("[scheduler] Skills evolution error:", e);
      }
    }

    // Reasoning check
    const lastReasoning = ctx.lastReasoningTimestamp ?? 0;
    if (heavyBudget > 0 && now - lastReasoning >= REASONING_INTERVAL_MS) {
      heavyBudget--;
      const taskId = activityBus.register("Reasoning Loop", "reasoning");
      console.log("[scheduler] Running reasoning loop...");
      const result = await runReasoningLoop();
      if (result.reasoned) {
        activityBus.complete(taskId, "Reflection complete");
        console.log("[scheduler] Reasoning loop complete.");
      } else {
        activityBus.complete(taskId, "Skipped");
        console.log("[scheduler] Reasoning loop skipped.");
      }
    }

    // Weekly pruning check
    const lastPrune = ctx.lastPruneTimestamp ?? 0;
    if (heavyBudget > 0 && now - lastPrune >= PRUNE_INTERVAL_MS) {
      heavyBudget--;
      const taskId = activityBus.register("Memory Pruning", "prune");
      console.log("[scheduler] Running weekly memory pruning...");
      const result = await pruneMemory();
      activityBus.complete(taskId, `${result.archived} archived, ${result.pruned} trimmed`);
      console.log(`[scheduler] Pruning complete: ${result.archived} archived, ${result.pruned} trimmed.`);
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
