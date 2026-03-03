/**
 * Terrance Continuity System
 *
 * Manages:
 *  - Identity injection (system prompt prepend)
 *  - Continuity primer (run before every reasoning task)
 *  - Session summarization (store after each session)
 *  - Memory retrieval (load before response generation)
 *
 * All persistent state lives outside model context in:
 *  ~/Library/Application Support/com.taisun.electron/
 *    terrance_context.json   — active goals + last session summary
 *    llm_usage.json          — cost/usage log (managed by llm_router.rs)
 *  nexus/memory/session_logs/ — per-session summaries (committed to repo)
 */

import { invoke } from "@tauri-apps/api/core";
import type { LlmRouterConfig, RouterResponse } from "@/lib/types";
import { buildRouterConfig } from "@/lib/llm-router";
import { getConfig } from "@/lib/api";

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

export async function loadIdentity(): Promise<string> {
  if (_identityCache) return _identityCache;
  try {
    _identityCache = await invoke<string>("read_identity_file");
  } catch {
    // Fallback inline identity if file read fails
    _identityCache = `You are Terrance — a persistent cognitive AI. ` +
      `Maintain continuity across sessions. Be direct, action-oriented, and transparent. ` +
      `Identity persists regardless of the underlying model.`;
  }
  return _identityCache;
}

// ─── Terrance Context (external memory store) ─────────────────────────────────

export interface TerranceContext {
  activeGoals: string[];
  lastSessionSummary: string | null;
  lastSessionTimestamp: number | null;
  relevantMemory: string[];
  lastConsolidationTimestamp?: number;
  lastReasoningTimestamp?: number;
}

export async function loadContext(): Promise<TerranceContext> {
  try {
    const ctx = await invoke<TerranceContext>("load_terrance_context");
    // Ensure arrays are always arrays (defensive against malformed JSON)
    return {
      ...ctx,
      activeGoals: ctx.activeGoals ?? [],
      relevantMemory: ctx.relevantMemory ?? [],
    };
  } catch {
    return {
      activeGoals: [],
      lastSessionSummary: null,
      lastSessionTimestamp: null,
      relevantMemory: [],
    };
  }
}

export async function saveContext(ctx: TerranceContext): Promise<void> {
  try {
    await invoke<void>("save_terrance_context", { context: ctx });
  } catch {
    // Non-fatal — context will reload from last good state on next session
  }
}

// ─── Continuity Primer ────────────────────────────────────────────────────────

/**
 * The continuity primer runs before any reasoning task.
 * It assembles identity + memory context into a system prompt.
 */
export async function buildSystemPrompt(ctx?: TerranceContext): Promise<string> {
  const [identity, context, skills] = await Promise.all([
    loadIdentity(),
    ctx ? Promise.resolve(ctx) : loadContext(),
    loadSkills(),
  ]);

  const parts: string[] = [identity, ""];

  // Skills block — inject active skill knowledge
  const activeSkills = (skills ?? []).filter((s) => s.status === "active" && s.content);
  if (activeSkills.length > 0) {
    parts.push("## Active Skills");
    parts.push("You have the following specialized capabilities loaded:\n");
    for (const skill of activeSkills) {
      parts.push(skill.content);
      parts.push("");
    }
  }

  // Continuity primer block
  parts.push("## Session Context");
  parts.push("Load Terrance identity. ✓");

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

  const memory = context.relevantMemory ?? [];
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
    .map((m) => `${m.role === "user" ? "User" : "Terrance"}: ${m.text}`)
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

function formatRelativeTime(ts: number): string {
  const diffMs = Date.now() - ts;
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  return `${Math.floor(diffHr / 24)}d ago`;
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

/**
 * Consolidates recent session logs (last 24h) into a single summary
 * and writes it to Letta via the n8n memory.write gateway.
 */
export async function consolidateRecentSessions(): Promise<{ consolidated: boolean; summary: string | null }> {
  try {
    const allLogs = await invoke<import("@/lib/types").SessionLogEntry[]>("list_session_logs");
    const cutoff = Date.now() - 24 * 60 * 60 * 1000;
    const recent = allLogs.filter((l) => l.timestamp >= cutoff);

    if (recent.length < 2) return { consolidated: false, summary: null };

    const summaries = recent.map((l, i) => `Session ${i + 1} (${new Date(l.timestamp).toLocaleString()}):\n${l.summary}`).join("\n\n");

    const consolidationPrompt =
      `You are Terrance, consolidating your recent session memories. ` +
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
    });

    const consolidated = resp.content.trim();
    const secret = cfg.n8n_webhook_secret ?? undefined;

    await fetch(N8N_INGRESS, {
      method: "POST",
      headers: buildIngressHeaders(secret),
      body: JSON.stringify({
        trace_id: `consolidation-${Date.now()}`,
        event: {
          type: "memory.write",
          payload: {
            content: `[Memory Consolidation — ${new Date().toLocaleDateString()}]\n${consolidated}`,
            mode: "append",
            block: "human",
          },
        },
      }),
    });

    // Re-load context to avoid overwriting concurrent changes
    const freshCtx = await loadContext();
    await saveContext({ ...freshCtx, lastConsolidationTimestamp: Date.now() });

    return { consolidated: true, summary: consolidated };
  } catch {
    return { consolidated: false, summary: null };
  }
}

// ─── Autonomous Reasoning Loop ────────────────────────────────────────────────

/**
 * Terrance reviews active goals against recent sessions and generates
 * a daily brief — a proactive self-reflection stored to memory.
 */
export async function runReasoningLoop(): Promise<{ reasoned: boolean; reflection: string | null }> {
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
      `You are Terrance, reviewing your recent activity.\n\n` +
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
    });

    const reflection = resp.content.trim();

    // Store as a session log
    const logEntry = {
      id: `reasoning-${Date.now()}`,
      timestamp: Date.now(),
      messageCount: 0,
      summary: reflection,
      provider: resp.provider,
      model: resp.model,
    };
    await invoke<void>("store_session_log", { entry: logEntry });

    // Write to Letta
    const secret = cfg.n8n_webhook_secret ?? undefined;
    await fetch(N8N_INGRESS, {
      method: "POST",
      headers: buildIngressHeaders(secret),
      body: JSON.stringify({
        trace_id: `reasoning-${Date.now()}`,
        event: {
          type: "memory.write",
          payload: {
            content: `[Daily Reflection — ${new Date().toLocaleDateString()}]\n${reflection}`,
            mode: "append",
            block: "human",
          },
        },
      }),
    });

    // Re-load context to avoid overwriting concurrent changes
    const freshCtx = await loadContext();
    await saveContext({ ...freshCtx, lastReasoningTimestamp: Date.now() });

    return { reasoned: true, reflection };
  } catch {
    return { reasoned: false, reflection: null };
  }
}

// ─── Re-export for convenience ────────────────────────────────────────────────

export type { LlmRouterConfig };
export { buildRouterConfig };
