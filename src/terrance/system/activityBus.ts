/**
 * Activity Bus + Task Registry
 *
 * Central event system for tracking all Amara background operations.
 * Uses Tauri's event system to bridge backend activity to the frontend UI.
 *
 * Usage:
 *   const taskId = activityBus.register("Daily Summary");
 *   activityBus.update(taskId, "running", "Generating briefing...");
 *   activityBus.complete(taskId, "Sent via Telegram");
 *
 * Frontend listens on "terrance://activity" for real-time updates.
 */

import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

// ─── Types ──────────────────────────────────────────────────────────────────

export type ActivityStatus = "pending" | "running" | "complete" | "error" | "cancelled";

export type ActivityType =
  | "scheduler"
  | "consolidation"
  | "reasoning"
  | "daily_summary"
  | "health_check"
  | "prune"
  | "custom_job"
  | "deliberation"
  | "research"
  | "compression"
  | "skills_evolution"
  | "diagnosis"
  | "auto_heal"
  | "session_save"
  | "telegram_bridge"
  | "subagent";

export interface ActivityEvent {
  id: string;
  type: ActivityType;
  name: string;
  status: ActivityStatus;
  progress?: number; // 0-100
  startedAt: number;
  completedAt?: number;
  detail?: string;
  /** Sub-steps for multi-phase operations (e.g. deliberation bull/bear/synthesis) */
  steps?: ActivityStep[];
}

export interface ActivityStep {
  label: string;
  status: ActivityStatus;
  detail?: string;
}

// ─── Task Registry ──────────────────────────────────────────────────────────

const _tasks = new Map<string, ActivityEvent>();
const _abortControllers = new Map<string, AbortController>();
let _idCounter = 0;

function generateId(): string {
  _idCounter++;
  return `task-${Date.now()}-${_idCounter}`;
}

async function emitActivity(event: ActivityEvent): Promise<void> {
  try {
    await emit("terrance://activity", event);
  } catch {
    // Non-fatal — UI may not be listening yet
  }
}

async function persistEntry(event: ActivityEvent): Promise<void> {
  if (event.status !== "complete" && event.status !== "error") return;
  try {
    await invoke<void>("store_activity_entry", {
      entry: {
        id: event.id,
        type: event.type,
        name: event.name,
        status: event.status,
        startedAt: event.startedAt,
        completedAt: event.completedAt ?? Date.now(),
        durationMs: (event.completedAt ?? Date.now()) - event.startedAt,
        detail: event.detail ?? null,
        outcome: event.status === "complete" ? "success" : "failure",
      },
    });
  } catch {
    // Persistence failure is non-fatal
  }
}

// ─── Public API ─────────────────────────────────────────────────────────────

export const activityBus = {
  /**
   * Register a new task. Returns the task ID.
   */
  register(
    name: string,
    type: ActivityType,
    opts?: { abortController?: AbortController; detail?: string },
  ): string {
    const id = generateId();
    const event: ActivityEvent = {
      id,
      type,
      name,
      status: "running",
      startedAt: Date.now(),
      detail: opts?.detail,
    };
    _tasks.set(id, event);
    if (opts?.abortController) {
      _abortControllers.set(id, opts.abortController);
    }
    emitActivity(event);
    return id;
  },

  /**
   * Update a running task's status or detail.
   */
  update(
    id: string,
    status: ActivityStatus,
    detail?: string,
    progress?: number,
  ): void {
    const task = _tasks.get(id);
    if (!task) return;
    task.status = status;
    if (detail !== undefined) task.detail = detail;
    if (progress !== undefined) task.progress = progress;
    if (status === "complete" || status === "error" || status === "cancelled") {
      task.completedAt = Date.now();
    }
    emitActivity(task);
    persistEntry(task);
  },

  /**
   * Mark a task as complete.
   */
  complete(id: string, detail?: string): void {
    this.update(id, "complete", detail);
  },

  /**
   * Mark a task as failed.
   */
  fail(id: string, detail?: string): void {
    this.update(id, "error", detail);
  },

  /**
   * Cancel a task. Calls AbortController if one was registered.
   */
  cancel(id: string): void {
    const controller = _abortControllers.get(id);
    if (controller) {
      controller.abort();
      _abortControllers.delete(id);
    }
    this.update(id, "cancelled", "Cancelled by user");
  },

  /**
   * Update sub-steps for multi-phase tasks (e.g. deliberation).
   */
  updateSteps(id: string, steps: ActivityStep[]): void {
    const task = _tasks.get(id);
    if (!task) return;
    task.steps = steps;
    emitActivity(task);
  },

  /**
   * Get all currently active (non-terminal) tasks.
   */
  getActiveTasks(): ActivityEvent[] {
    return Array.from(_tasks.values()).filter(
      (t) => t.status === "pending" || t.status === "running",
    );
  },

  /**
   * Get all tasks (including completed) — for UI timeline.
   * Returns most recent first, capped at 50.
   */
  getRecentTasks(limit = 50): ActivityEvent[] {
    return Array.from(_tasks.values())
      .sort((a, b) => b.startedAt - a.startedAt)
      .slice(0, limit);
  },

  /**
   * Clean up old completed tasks from memory (keeps last 100).
   */
  prune(): void {
    const all = Array.from(_tasks.entries())
      .sort(([, a], [, b]) => b.startedAt - a.startedAt);
    if (all.length > 100) {
      for (const [id] of all.slice(100)) {
        _tasks.delete(id);
        _abortControllers.delete(id);
      }
    }
  },
};
