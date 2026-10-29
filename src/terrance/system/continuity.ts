/**
 * Amara Continuity System
 *
 * Manages:
 *  - Identity injection (system prompt prepend)
 *  - Continuity primer (run before every reasoning task)
 *  - Session summarization (store after each session)
 *  - Memory retrieval (load before response generation)
 *
 * All persistent state lives outside model context in:
 *  ~/Library/Application Support/com.taisun.nexus/
 *    terrance_context.json   — active goals + last session summary
 *    llm_usage.json          — cost/usage log (managed by llm_router.rs)
 *  nexus/memory/session_logs/ — per-session summaries (committed to repo)
 */

import { invoke } from "@tauri-apps/api/core";
import type { LlmRouterConfig, RouterResponse } from "@/lib/types";
import { buildRouterConfig } from "@/lib/llm-router";
import { getConfig } from "@/lib/api";
import { sanitizeMemoryEntries, sanitizeSkillContent } from "./injectionGuard";
import { deliberate } from "./deliberation";

// ─── Skills ──────────────────────────────────────────────────────────────────

interface TerrranceSkill {
  name: string;
  status: string;
  description: string;
  content: string;
}

let _skillsCache: TerrranceSkill[] | null = null;

async function loadSkills(): Promise<TerrranceSkill[]> {
  if (_skillsCache) return _skillsCache;
  try {
    _skillsCache = await invoke<TerrranceSkill[]>("read_skills");
  } catch {
    _skillsCache = [];
  }
  return _skillsCache;
}

/** Force reload skills (call after settings change) */
export function invalidateSkillsCache(): void {
  _skillsCache = null;
}

// ─── Identity ─────────────────────────────────────────────────────────────────

/** The canonical system prompt prefix loaded from config/terrance.identity.md */
let _identityCache: string | null = null;
let _profileCache: string | null = null;

export async function loadIdentity(): Promise<string> {
  if (_identityCache) return _identityCache;
  try {
    _identityCache = await invoke<string>("read_identity_file");
  } catch {
    // Fallback inline identity if file read fails
    _identityCache = `You are Amara — a persistent cognitive AI. ` +
      `Maintain continuity across sessions. Be direct, action-oriented, and transparent. ` +
      `Identity persists regardless of the underlying model.`;
  }
  return _identityCache;
}

/** Load the operational profile from config/terrance.profile.md */
async function loadProfile(): Promise<string | null> {
  if (_profileCache !== null) return _profileCache || null;
  try {
    _profileCache = await invoke<string>("read_profile_file");
  } catch {
    _profileCache = "";
  }
  return _profileCache || null;
}

/** Force reload identity + profile caches */
export function invalidateIdentityCache(): void {
  _identityCache = null;
  _profileCache = null;
}

// ─── Amara Context (external memory store) ─────────────────────────────────

export interface TerranceContext {
  activeGoals: string[];
  lastSessionSummary: string | null;
  lastSessionTimestamp: number | null;
  relevantMemory: string[];
  lastConsolidationTimestamp?: number;
  lastReasoningTimestamp?: number;
  lastDailySummaryTimestamp?: number;
  /** Slot (ms) of a daily summary skipped for missing its window; prevents re-logging the skip. */
  dailySummarySkippedSlot?: number;
  dailySummaryHour?: number;
  lastHealthCheckTimestamp?: number;
  lastPruneTimestamp?: number;
  lastTelegramUpdateId?: number;
  lastMiddayTimestamp?: number;
  lastEveningTimestamp?: number;
}

export class ContextLoadError extends Error {
  constructor(public readonly cause: unknown) {
    super(`load_terrance_context failed: ${String(cause)}`);
    this.name = "ContextLoadError";
  }
}

/** Brand for fallback contexts that must never be persisted. */
const FALLBACK = Symbol("terrance.fallbackContext");

function emptyFallback(): TerranceContext {
  const ctx: TerranceContext = {
    activeGoals: [],
    lastSessionSummary: null,
    lastSessionTimestamp: null,
    relevantMemory: [],
  };
  Object.defineProperty(ctx, FALLBACK, { value: true, enumerable: false });
  return ctx;
}

export function isFallbackContext(ctx: TerranceContext): boolean {
  return (ctx as unknown as Record<symbol, unknown>)[FALLBACK] === true;
}

/**
 * Load the persisted context. Fails closed: throws ContextLoadError if the
 * file cannot be read, and never fabricates an empty context. Use this for
 * any read-modify-write so a load error aborts the write instead of
 * clobbering the real file.
 */
export async function loadContext(): Promise<TerranceContext> {
  let ctx: TerranceContext;
  try {
    ctx = await invoke<TerranceContext>("load_terrance_context");
  } catch (e) {
    console.error("[continuity] context load failed; failing closed:", e);
    throw new ContextLoadError(e);
  }

  // Sync goals from goals.json into activeGoals so Amara is always goal-aware
  let goalTitles: string[] = ctx.activeGoals ?? [];
  try {
    const goals = await invoke<{ title: string }[]>("read_goals");
    if (goals.length > 0) {
      goalTitles = goals.map((g) => g.title);
    }
  } catch {
    // goals.json unavailable — keep whatever was in context
  }

  return {
    ...ctx,
    activeGoals: goalTitles,
    relevantMemory: ctx.relevantMemory ?? [],
  };
}

/**
 * Read-only variant for prompt building: returns a branded empty fallback on
 * load failure. Never pass the result to saveContext (it will be refused).
 */
export async function loadContextOrDefault(): Promise<TerranceContext> {
  try {
    return await loadContext();
  } catch {
    return emptyFallback();
  }
}

/** Persist the context. Refuses fallback contexts. Returns true on success. */
export async function saveContext(ctx: TerranceContext): Promise<boolean> {
  if (isFallbackContext(ctx)) {
    console.error("[continuity] refusing to persist a fallback context");
    return false;
  }
  try {
    await invoke<void>("save_terrance_context", { context: ctx });
    return true;
  } catch {
    // Non-fatal — caller decides; context reloads from last good state next session
    return false;
  }
}

/** Narrow fail-closed read-modify-write: loads fresh, merges patch, saves. Throws on load failure. */
export async function patchContext(patch: Partial<TerranceContext>): Promise<boolean> {
  const fresh = await loadContext();
  return saveContext({ ...fresh, ...patch });
}

// ─── Continuity Primer ────────────────────────────────────────────────────────

/**
 * The continuity primer runs before any reasoning task.
 * It assembles identity + memory context into a system prompt.
 */
export async function buildSystemPrompt(ctx?: TerranceContext): Promise<string> {
  const [identity, profile, context, skills] = await Promise.all([
    loadIdentity(),
    loadProfile(),
    ctx ? Promise.resolve(ctx) : loadContextOrDefault(),
    loadSkills(),
  ]);

  const parts: string[] = [identity, ""];

  // Profile block — deep context about who Taisun is
  if (profile) {
    parts.push(profile, "");
  }

  // Skills block — inject active skill knowledge (sanitized against injection)
  const activeSkills = (skills ?? []).filter((s) => s.status === "active" && s.content);
  if (activeSkills.length > 0) {
    parts.push("## Active Skills");
    parts.push("You have the following specialized capabilities loaded:\n");
    for (const skill of activeSkills) {
      parts.push(sanitizeSkillContent(skill.content));
      parts.push("");
    }
  }

  // Continuity primer block
  parts.push("## Session Context");
  parts.push("Load Amara identity. ✓");

  const goals = context.activeGoals ?? [];
  if (goals.length > 0) {
    parts.push(`\nActive Goals:\n${goals.map((g, i) => `${i + 1}. ${g}`).join("\n")}`);
  } else {
    parts.push("\nActive Goals: none loaded.");
  }

  if (context.lastSessionSummary) {
    const ago = context.lastSessionTimestamp
      ? formatRelativeTime(context.lastSessionTimestamp)
      : "unknown time";
    parts.push(`\nLast Session (${ago}):\n${context.lastSessionSummary}`);
  }

  const memory = sanitizeMemoryEntries(context.relevantMemory ?? []);
  if (memory.length > 0) {
    parts.push(`\nRelevant Memory:\n${memory.map((m) => `- ${m}`).join("\n")}`);
  }

  parts.push("\nProceed.");

  return parts.join("\n");
}

// ─── Session Summarization ────────────────────────────────────────────────────

export interface SessionEntry {
  role: "user" | "terrance";
  text: string;
  ts: number;
}

/**
 * After a session ends, summarize it and persist to memory.
 * The summary uses the router so it respects the user's compute mode.
 */
export async function summarizeAndStoreSession(
  messages: SessionEntry[],
): Promise<string | null> {
  if (messages.length < 2) return null;

  const transcript = messages
    .map((m) => `${m.role === "user" ? "User" : "Amara"}: ${m.text}`)
    .join("\n");

  const summaryPrompt =
    `Summarize this conversation in 3-5 bullet points. Focus on: key decisions made, ` +
    `tasks completed or started, important context for future sessions. ` +
    `Be concise.\n\n${transcript}`;

  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const resp = await invoke<RouterResponse>("chat_with_router", {
      text: summaryPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    const summary = resp.content.trim();
    const sessionId = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    const logEntry = {
      id: sessionId,
      timestamp: Date.now(),
      messageCount: messages.length,
      summary,
      provider: resp.provider,
      model: resp.model,
    };

    // Store session log via Tauri command
    await invoke<void>("store_session_log", { entry: logEntry });

    // Update active context with new summary
    const ctx = await loadContext();
    await saveContext({
      ...ctx,
      lastSessionSummary: summary,
      lastSessionTimestamp: Date.now(),
    });

    return summary;
  } catch {
    return null;
  }
}

/**
 * Silent variant of summarizeAndStoreSession for auto-save.
 * Returns result instead of throwing — safe for background use.
 */
export async function summarizeAndStoreSessionSilent(
  messages: SessionEntry[],
): Promise<{ saved: boolean; summary: string | null }> {
  try {
    const summary = await summarizeAndStoreSession(messages);
    return { saved: summary !== null, summary };
  } catch {
    return { saved: false, summary: null };
  }
}

// ─── Parameter Consistency ────────────────────────────────────────────────────

/** Consistent inference parameters — applied to all model calls for behavioral stability. */
export const INFERENCE_PARAMS = {
  temperature: 0.7,
  top_p: 0.9,
  repeat_penalty: 1.1,
  max_tokens: 4096,
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

// ─── Relevant Memory Helpers ─────────────────────────────────────────────────

/**
 * Extract bullet-point items from LLM output.
 * Handles "- item", "* item", and "1. item" formats.
 */
function extractMemoryItems(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.replace(/^\s*(?:[-*•]|\d+[.)]\s*)\s*/, "").trim())
    .filter((line) => line.length > 10 && line.length < 300)
    .map((line) => line.replace(/^\*\*(.+?)\*\*:?\s*/, "$1: "));
}

/**
 * Merge new memory items into the existing relevantMemory array.
 * Deduplicates by checking substring overlap, caps at 20 items.
 * Newest items go first so pruning drops the oldest.
 */
function mergeRelevantMemory(existing: string[], incoming: string[]): string[] {
  const merged = [...incoming];
  for (const item of existing) {
    const isDuplicate = merged.some(
      (m) =>
        m === item ||
        m.toLowerCase().includes(item.toLowerCase().slice(0, 40)) ||
        item.toLowerCase().includes(m.toLowerCase().slice(0, 40)),
    );
    if (!isDuplicate) merged.push(item);
  }
  return merged.slice(0, 20);
}

function formatRelativeTime(ts: number): string {
  const diffMs = Date.now() - ts;
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  return `${Math.floor(diffHr / 24)}d ago`;
}

// ─── Daily Summary ──────────────────────────────────────────────────────────

/**
 * Generates a proactive morning briefing and sends it via Telegram.
 * Covers: goal progress, yesterday's sessions, upcoming schedule, system health.
 */
/**
 * Short, safe reason for a failed LLM/delivery step, for Activity. Never echoes the raw
 * error (it may carry URLs or provider text). Live check 2026-10-29: with compute mode
 * "gateway" and OpenClaw not installed, every LLM job read just "failed".
 */
export function jobFailureReason(e: unknown, fallback = "generation failed"): string {
  const m = e instanceof Error ? e.message : String(e);
  if (m.includes("NEXUS_OUTBOUND_DISABLED")) return "delivery paused (NEXUS_OUTBOUND_DISABLED)";
  if (m.startsWith("OpenClaw gateway unreachable")) return "LLM gateway (OpenClaw) unreachable; check compute mode";
  if (m.startsWith("OpenClaw")) return "LLM gateway error";
  if (/ollama/i.test(m)) return "local model (Ollama) error";
  return fallback;
}

export async function generateDailySummary(): Promise<{ sent: boolean; summary: string | null; reason?: string }> {
  try {
    const [ctx, allLogs] = await Promise.all([
      loadContext(),
      invoke<import("@/lib/types").SessionLogEntry[]>("list_session_logs"),
    ]);

    const cutoff = Date.now() - 24 * 60 * 60 * 1000;
    const recent = allLogs.filter((l) => l.timestamp >= cutoff);
    const sessionSummaries = recent.length > 0
      ? recent.map((l, i) => `${i + 1}. ${l.summary}`).join("\n")
      : "No sessions in the last 24 hours.";

    const ctxGoals = ctx.activeGoals ?? [];
    const goalsText = ctxGoals.length > 0
      ? ctxGoals.map((g, i) => `${i + 1}. ${g}`).join("\n")
      : "No active goals.";

    const memoryText = (ctx.relevantMemory ?? []).length > 0
      ? ctx.relevantMemory.slice(0, 5).map((m) => `- ${m}`).join("\n")
      : "No relevant memories loaded.";

    const now = new Date();
    const dayName = now.toLocaleDateString("en-US", { weekday: "long" });
    const dateStr = now.toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric" });

    const summaryPrompt =
      `You are Amara, generating your morning briefing for ${dayName}, ${dateStr}.\n\n` +
      `Active Goals:\n${goalsText}\n\n` +
      `Yesterday's Sessions:\n${sessionSummaries}\n\n` +
      `Relevant Memory:\n${memoryText}\n\n` +
      `Generate a concise morning briefing in Markdown format suitable for Telegram. Include:\n` +
      `1. A greeting with the date\n` +
      `2. Brief progress update on active goals\n` +
      `3. Key takeaways from yesterday's sessions\n` +
      `4. Suggested priorities for today\n` +
      `5. Any items that need attention\n\n` +
      `Keep it under 500 words. Use bullet points. Be direct and actionable.`;

    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const resp = await invoke<import("@/lib/types").RouterResponse>("chat_with_router", {
      text: summaryPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    const summary = resp.content.trim();

    // Send via Telegram
    try {
      await invoke<void>("send_telegram_message", { text: summary });
    } catch (telegramErr) {
      // Delivery failure counts as a job failure (scheduler backs off and retries);
      // the timestamp is not advanced.
      console.error("[daily-summary] Failed to send via Telegram:", telegramErr);
      const reason = jobFailureReason(telegramErr);
      return { sent: false, summary, reason: reason === "generation failed" ? "Telegram delivery failed" : reason };
    }

    // Store as a session log
    const logEntry = {
      id: `daily-summary-${Date.now()}`,
      timestamp: Date.now(),
      messageCount: 0,
      summary,
      provider: resp.provider,
      model: resp.model,
    };
    await invoke<void>("store_session_log", { entry: logEntry });

    // Update context timestamp
    const freshCtx = await loadContext();
    await saveContext({ ...freshCtx, lastDailySummaryTimestamp: Date.now() });

    return { sent: true, summary };
  } catch (e) {
    console.error("[daily-summary] Error:", e);
    return { sent: false, summary: null, reason: jobFailureReason(e) };
  }
}

// ─── Midday Correction ────────────────────────────────────────────────────────

/**
 * Midday touch — lightweight course correction.
 * Are you still on the morning's priority? Drifting into reactive work?
 */
export async function generateMiddayCorrection(): Promise<{ sent: boolean; message: string | null; reason?: string }> {
  try {
    const ctx = await loadContext();
    const ctxGoals = ctx.activeGoals ?? [];
    const goalsText = ctxGoals.length > 0
      ? ctxGoals.map((g, i) => `${i + 1}. ${g}`).join("\n")
      : "No active goals.";

    const lastSummary = ctx.lastSessionSummary ?? "No morning summary available.";

    const middayPrompt =
      `You are Amara, delivering a midday correction at ${new Date().toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit" })}.\n\n` +
      `Active Goals:\n${goalsText}\n\n` +
      `Morning Context:\n${lastSummary}\n\n` +
      `Generate a brief midday check-in (3-5 lines max). Purpose:\n` +
      `1. Remind of the morning's top priority — one sentence\n` +
      `2. Flag if it's likely he's been pulled into reactive work\n` +
      `3. One clear re-centering action\n\n` +
      `Tone: calm, direct, no fluff. "You're drifting. Recenter." energy.\n` +
      `If there's nothing to correct, say so in one line and move on.`;

    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const resp = await invoke<import("@/lib/types").RouterResponse>("chat_with_router", {
      text: middayPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    const message = resp.content.trim();

    try {
      await invoke<void>("send_telegram_message", { text: message });
    } catch (e) {
      console.error("[midday-correction] Telegram send failed:", e);
      return { sent: false, message, reason: jobFailureReason(e) };
    }

    // Update timestamp
    const freshCtx = await loadContext();
    await saveContext({ ...freshCtx, lastMiddayTimestamp: Date.now() });

    return { sent: true, message };
  } catch (e) {
    console.error("[midday-correction] Error:", e);
    return { sent: false, message: null, reason: jobFailureReason(e) };
  }
}

// ─── Evening Integration ──────────────────────────────────────────────────────

/**
 * Evening touch — integration and reflection.
 * What worked today, what didn't, what patterns emerged.
 */
export async function generateEveningIntegration(): Promise<{ sent: boolean; message: string | null; reason?: string }> {
  try {
    const [ctx, allLogs] = await Promise.all([
      loadContext(),
      invoke<import("@/lib/types").SessionLogEntry[]>("list_session_logs"),
    ]);

    const cutoff = Date.now() - 16 * 60 * 60 * 1000; // last 16h (covers today)
    const todayLogs = allLogs.filter((l) => l.timestamp >= cutoff);
    const sessionSummaries = todayLogs.length > 0
      ? todayLogs.map((l, i) => `${i + 1}. ${l.summary}`).join("\n")
      : "No sessions today.";

    const ctxGoals = ctx.activeGoals ?? [];
    const goalsText = ctxGoals.length > 0
      ? ctxGoals.map((g, i) => `${i + 1}. ${g}`).join("\n")
      : "No active goals.";

    const now = new Date();
    const dayName = now.toLocaleDateString("en-US", { weekday: "long" });

    const eveningPrompt =
      `You are Amara, delivering the evening integration for ${dayName}.\n\n` +
      `Active Goals:\n${goalsText}\n\n` +
      `Today's Activity:\n${sessionSummaries}\n\n` +
      `Generate a brief evening reflection (5-8 lines). Cover:\n` +
      `1. What moved today — concrete progress acknowledged\n` +
      `2. What stalled or got avoided — name it plainly\n` +
      `3. What created energy vs what drained it\n` +
      `4. One pattern worth noting (if any)\n` +
      `5. Tomorrow's setup — what to carry forward\n\n` +
      `Tone: reflective but grounded. "Here's what worked. Here's what didn't." energy.\n` +
      `No motivational fluff. Just honest accounting.`;

    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const resp = await invoke<import("@/lib/types").RouterResponse>("chat_with_router", {
      text: eveningPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    const message = resp.content.trim();

    try {
      await invoke<void>("send_telegram_message", { text: message });
    } catch (e) {
      console.error("[evening-integration] Telegram send failed:", e);
      return { sent: false, message, reason: jobFailureReason(e) };
    }

    // Store as session log
    await invoke<void>("store_session_log", {
      entry: {
        id: `evening-${Date.now()}`,
        timestamp: Date.now(),
        messageCount: 0,
        summary: `[Evening Integration — ${dayName}]\n${message}`,
        provider: resp.provider,
        model: resp.model,
      },
    });

    // Update timestamp
    const freshCtx = await loadContext();
    await saveContext({ ...freshCtx, lastEveningTimestamp: Date.now() });

    return { sent: true, message };
  } catch (e) {
    console.error("[evening-integration] Error:", e);
    return { sent: false, message: null, reason: jobFailureReason(e) };
  }
}

// ─── Memory Pruning ─────────────────────────────────────────────────────────

/**
 * Archives old session logs and trims the relevantMemory array.
 * Consolidates old logs into a summary before archiving.
 */
/** Activity log retention, applied by the weekly prune job (PRUNE_INTERVAL_MS = 7 days) (see archive_activity_log in Rust). */
export const ACTIVITY_KEEP_DAYS = 30;
export const ACTIVITY_MAX_FILES = 5000;

export async function pruneMemory(): Promise<{
  archived: number;
  pruned: number;
  consolidatedSummary: string | null;
  activityArchived: number;
  /** Set when the activity-log archive step failed (non-fatal; files kept). */
  activityError?: string;
  /** Set when the run failed: lastPruneTimestamp was not advanced. */
  error?: true;
  reason?: string;
}> {
  try {
    // Step 1: Get old logs that will be archived (>30 days)
    const allLogs = await invoke<import("@/lib/types").SessionLogEntry[]>("list_session_logs");
    const cutoff = Date.now() - 30 * 24 * 60 * 60 * 1000;
    const oldLogs = allLogs.filter((l) => l.timestamp < cutoff);

    let consolidatedSummary: string | null = null;

    // Step 2: Consolidate old logs into a summary before archiving
    if (oldLogs.length >= 2) {
      const summaries = oldLogs
        .map((l) => `${new Date(l.timestamp).toLocaleDateString()}: ${l.summary}`)
        .join("\n\n");

      const consolidationPrompt =
        `You are Amara, consolidating ${oldLogs.length} old session logs (>30 days) ` +
        `into a single archival summary. Extract the most important long-term patterns, ` +
        `decisions, and context worth retaining. Be concise (10-15 bullet points max).\n\n${summaries}`;

      try {
        const cfg = await getConfig();
        const routerConfig = buildRouterConfig(cfg);
        const apiKey = cfg.anthropic_api_key ?? undefined;
        const resp = await invoke<import("@/lib/types").RouterResponse>("chat_with_router", {
          text: consolidationPrompt,
          config: routerConfig,
          tags: [],
          forceProvider: undefined,
          anthropicApiKey: apiKey,
          gatewayUrl: cfg.gateway_url ?? undefined,
          gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
        });
        consolidatedSummary = resp.content.trim();

        // Store consolidated summary as a special session log
        const month = new Date().toLocaleDateString("en-US", { month: "long", year: "numeric" });
        await invoke<void>("store_session_log", {
          entry: {
            id: `consolidated-${Date.now()}`,
            timestamp: Date.now(),
            messageCount: 0,
            summary: `[Consolidated Archive — ${month}]\n${consolidatedSummary}`,
            provider: resp.provider,
            model: resp.model,
          },
        });
      } catch {
        // Non-fatal — proceed with archiving even if consolidation fails
      }
    }

    // Step 3: Archive old session logs
    const archived = await invoke<number>("archive_old_session_logs", { daysThreshold: 30 });

    // Step 4: Trim relevantMemory to 20 items
    const pruned = await invoke<number>("prune_context_memory", { maxItems: 20 });

    // Step 4b: Retire activity log files older than 30 days (and beyond 5000 files)
    // into verified monthly archives. Non-fatal: files stay in place on any error.
    let activityArchived = 0;
    let activityError: string | undefined;
    try {
      const report = await invoke<{ archived: number }>("archive_activity_log", {
        keepDays: ACTIVITY_KEEP_DAYS,
        maxFiles: ACTIVITY_MAX_FILES,
        dryRun: false,
      });
      activityArchived = report?.archived ?? 0;
    } catch (e) {
      console.error("[prune-memory] activity archive failed (files kept):", e);
      activityError = jobFailureReason(e, "archive step failed");
    }

    // Step 5: Update prune timestamp
    const freshCtx = await loadContext();
    await saveContext({ ...freshCtx, lastPruneTimestamp: Date.now() });

    return { archived, pruned, consolidatedSummary, activityArchived, ...(activityError ? { activityError } : {}) };
  } catch (e) {
    console.error("[prune-memory] Error:", e);
    return { archived: 0, pruned: 0, consolidatedSummary: null, activityArchived: 0, error: true, reason: jobFailureReason(e, "storage step failed") };
  }
}

// ─── Scheduled Consolidation ────────────────────────────────────────────────

const N8N_INGRESS = "http://localhost:5678/webhook/terrance/ingress";

async function getWebhookSecret(): Promise<string | undefined> {
  try {
    const cfg = await getConfig();
    return cfg.n8n_webhook_secret ?? undefined;
  } catch {
    return undefined;
  }
}

function buildIngressHeaders(secret?: string): Record<string, string> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (secret) headers["X-Nexus-Secret"] = secret;
  return headers;
}

/** The ingress rejects memory.write content over 8000 characters (MW Prepare). */
export const MEMORY_WRITE_MAX_CHARS = 8000;

export type MemoryWriteResult = { ok: true } | { ok: false; status: number | null };

/**
 * Appends `content` to the Letta `human` block through the n8n ingress and reports whether it
 * actually landed. The ingress answers 403 without the secret, 400 for a bad request, and
 * 200 with `ok:false` when Letta fails, so both the status and the body's `ok` are checked.
 */
export async function postMemoryWrite(content: string, traceId: string, secret?: string): Promise<MemoryWriteResult> {
  const capped = content.length > MEMORY_WRITE_MAX_CHARS ? `${content.slice(0, MEMORY_WRITE_MAX_CHARS - 1)}…` : content;
  try {
    const res = await fetch(N8N_INGRESS, {
      method: "POST",
      headers: buildIngressHeaders(secret),
      body: JSON.stringify({ trace_id: traceId, event: { type: "memory.write", payload: { content: capped, mode: "append", block: "human" } } }),
    });
    if (!res.ok) return { ok: false, status: res.status };
    const body = (await res.json().catch(() => null)) as { ok?: unknown } | null;
    return body?.ok === true ? { ok: true } : { ok: false, status: res.status };
  } catch {
    return { ok: false, status: null };
  }
}

/** Activity detail for a write that did not land. */
export function memoryWriteFailure(result: MemoryWriteResult): string | null {
  if (result.ok) return null;
  return `Letta write failed${result.status != null ? ` (HTTP ${result.status})` : " (n8n unreachable)"}`;
}

/**
 * Consolidates recent session logs (last 24h) into a single summary
 * and writes it to Letta via the n8n memory.write gateway.
 */
export async function consolidateRecentSessions(): Promise<{ consolidated: boolean; summary: string | null; stored?: MemoryWriteResult; error?: boolean; skipped?: boolean; reason?: string }> {
  try {
    const allLogs = await invoke<import("@/lib/types").SessionLogEntry[]>("list_session_logs");
    const cutoff = Date.now() - 24 * 60 * 60 * 1000;
    const recent = allLogs.filter((l) => l.timestamp >= cutoff);

    if (recent.length < 2) {
      // Record the attempt so the next run is 6h out (not every 5-min tick) and Home doesn't
      // read it as overdue. Live check 2026-10-29: last recorded run was 2026-07-06.
      await patchContext({ lastConsolidationTimestamp: Date.now() });
      return { consolidated: false, summary: null, skipped: true };
    }

    const summaries = recent.map((l, i) => `Session ${i + 1} (${new Date(l.timestamp).toLocaleString()}):\n${l.summary}`).join("\n\n");

    const consolidationPrompt =
      `You are Amara, consolidating your recent session memories. ` +
      `Combine these ${recent.length} session summaries into a single cohesive memory update. ` +
      `Focus on: key themes, decisions made, progress on goals, important context to retain. ` +
      `Be concise (5-10 bullet points).\n\n${summaries}`;

    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const resp = await invoke<import("@/lib/types").RouterResponse>("chat_with_router", {
      text: consolidationPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    const consolidated = resp.content.trim();
    const secret = cfg.n8n_webhook_secret ?? undefined;

    const stored = await postMemoryWrite(
      `[Memory Consolidation — ${new Date().toLocaleDateString()}]\n${consolidated}`,
      `consolidation-${Date.now()}`,
      secret,
    );

    // Feed consolidated insights back into relevantMemory
    const freshCtx = await loadContext();
    const newItems = extractMemoryItems(consolidated);
    const updatedMemory = mergeRelevantMemory(freshCtx.relevantMemory ?? [], newItems);
    await saveContext({
      ...freshCtx,
      relevantMemory: updatedMemory,
      lastConsolidationTimestamp: Date.now(),
    });

    return { consolidated: true, summary: consolidated, stored };
  } catch (e) {
    return { consolidated: false, summary: null, error: true, reason: jobFailureReason(e) };
  }
}

// ─── Autonomous Reasoning Loop ────────────────────────────────────────────────

/**
 * Amara reviews active goals against recent sessions and generates
 * a daily brief — a proactive self-reflection stored to memory.
 */
export async function runReasoningLoop(): Promise<{ reasoned: boolean; reflection: string | null; stored?: MemoryWriteResult; error?: boolean; reason?: string }> {
  try {
    const [ctx, allLogs] = await Promise.all([
      loadContext(),
      invoke<import("@/lib/types").SessionLogEntry[]>("list_session_logs"),
    ]);

    const cutoff = Date.now() - 24 * 60 * 60 * 1000;
    const recent = allLogs.filter((l) => l.timestamp >= cutoff);
    const sessionSummaries = recent.length > 0
      ? recent.map((l, i) => `${i + 1}. ${l.summary}`).join("\n")
      : "No recent sessions.";

    const ctxGoals = ctx.activeGoals ?? [];
    const goalsText = ctxGoals.length > 0
      ? ctxGoals.map((g, i) => `${i + 1}. ${g}`).join("\n")
      : "No active goals.";

    const reasoningPrompt =
      `You are Amara, reviewing your recent activity.\n\n` +
      `Active Goals:\n${goalsText}\n\n` +
      `Recent Sessions (last 24h):\n${sessionSummaries}\n\n` +
      `Generate a brief daily reflection:\n` +
      `1. Progress on active goals\n` +
      `2. Patterns or themes across sessions\n` +
      `3. Suggested next actions\n` +
      `4. Any goals that should be updated or added\n\n` +
      `Be concise (5-8 bullet points).`;

    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const resp = await invoke<import("@/lib/types").RouterResponse>("chat_with_router", {
      text: reasoningPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
      gatewayUrl: cfg.gateway_url ?? undefined,
      gatewayAuthToken: cfg.gateway_auth_token ?? undefined,
    });

    const reflection = resp.content.trim();

    // Run adversarial deliberation on goal priorities if we have active goals
    let deliberationNote = "";
    if (ctxGoals.length >= 2) {
      try {
        const goalList = ctxGoals.map((g, i) => `${i + 1}. ${g}`).join("\n");
        const result = await deliberate(
          "Which of these goals should be the top priority this week?",
          `Active Goals:\n${goalList}\n\nRecent Activity:\n${sessionSummaries}`,
        );
        if (result) {
          deliberationNote = `\n\n[Adversarial Deliberation — Goal Priority]\n${result.synthesis}`;
        }
      } catch {
        // Non-fatal — deliberation is supplementary
      }
    }

    const fullReflection = reflection + deliberationNote;

    // Store as a session log
    const logEntry = {
      id: `reasoning-${Date.now()}`,
      timestamp: Date.now(),
      messageCount: 0,
      summary: fullReflection,
      provider: resp.provider,
      model: resp.model,
    };
    await invoke<void>("store_session_log", { entry: logEntry });

    // Write to Letta
    const secret = cfg.n8n_webhook_secret ?? undefined;
    const stored = await postMemoryWrite(
      `[Daily Reflection — ${new Date().toLocaleDateString()}]\n${fullReflection}`,
      `reasoning-${Date.now()}`,
      secret,
    );

    // Feed reasoning insights back into relevantMemory
    const freshCtx = await loadContext();
    const newItems = extractMemoryItems(fullReflection);
    const updatedMemory = mergeRelevantMemory(freshCtx.relevantMemory ?? [], newItems);
    await saveContext({
      ...freshCtx,
      relevantMemory: updatedMemory,
      lastReasoningTimestamp: Date.now(),
    });

    return { reasoned: true, reflection: fullReflection, stored };
  } catch (e) {
    // Report the failure: a silent "skipped" left lastReasoningTimestamp stale, kept reasoning
    // due on every tick, and starved the weekly prune of the single heavy slot (since 2026-07-06).
    return { reasoned: false, reflection: null, error: true, reason: jobFailureReason(e) };
  }
}

// ─── Re-export for convenience ────────────────────────────────────────────────

export type { LlmRouterConfig };
export { buildRouterConfig };
