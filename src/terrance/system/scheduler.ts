/**
 * Terrance Scheduler
 *
 * Manages periodic background tasks:
 *  - Memory consolidation (every 6 hours)
 *  - Autonomous reasoning loop (every 12 hours)
 *
 * Uses timestamps persisted in terrance_context.json to avoid
 * duplicate runs after app restart.
 */

import { loadContext, consolidateRecentSessions, runReasoningLoop } from "./continuity";

const CONSOLIDATION_INTERVAL_MS = 6 * 60 * 60 * 1000;  // 6 hours
const REASONING_INTERVAL_MS = 12 * 60 * 60 * 1000;      // 12 hours
const CHECK_INTERVAL_MS = 5 * 60 * 1000;                 // Check every 5 minutes

let checkIntervalId: ReturnType<typeof setInterval> | null = null;
let startupTimerId: ReturnType<typeof setTimeout> | null = null;
let running = false;

async function tick() {
  if (running) return;
  running = true;

  try {
    const ctx = await loadContext();
    const now = Date.now();

    // Consolidation check
    const lastConsolidation = ctx.lastConsolidationTimestamp ?? 0;
    if (now - lastConsolidation >= CONSOLIDATION_INTERVAL_MS) {
      console.log("[scheduler] Running memory consolidation...");
      const result = await consolidateRecentSessions();
      if (result.consolidated) {
        console.log("[scheduler] Consolidation complete.");
      } else {
        console.log("[scheduler] Consolidation skipped (not enough sessions).");
      }
    }

    // Reasoning check
    const lastReasoning = ctx.lastReasoningTimestamp ?? 0;
    if (now - lastReasoning >= REASONING_INTERVAL_MS) {
      console.log("[scheduler] Running reasoning loop...");
      const result = await runReasoningLoop();
      if (result.reasoned) {
        console.log("[scheduler] Reasoning loop complete.");
      } else {
        console.log("[scheduler] Reasoning loop skipped.");
      }
    }
  } catch (e) {
    console.error("[scheduler] Error:", e);
  } finally {
    running = false;
  }
}

export function startScheduler(): void {
  if (checkIntervalId) return;
  console.log("[scheduler] Started (consolidation=6h, reasoning=12h).");
  // Run first tick after a short delay to let the app fully initialize
  startupTimerId = setTimeout(tick, 30_000);
  checkIntervalId = setInterval(tick, CHECK_INTERVAL_MS);
}

export function stopScheduler(): void {
  if (startupTimerId) { clearTimeout(startupTimerId); startupTimerId = null; }
  if (checkIntervalId) {
    clearInterval(checkIntervalId);
    checkIntervalId = null;
  }
  console.log("[scheduler] Stopped.");
}
