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
}

export async function loadContext(): Promise<TerranceContext> {
  try {
    return await invoke<TerranceContext>("load_terrance_context");
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
  const activeSkills = skills.filter((s) => s.status === "active" && s.content);
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

  if (context.activeGoals.length > 0) {
    parts.push(`\nActive Goals:\n${context.activeGoals.map((g, i) => `${i + 1}. ${g}`).join("\n")}`);
  } else {
    parts.push("\nActive Goals: none loaded.");
  }

  if (context.lastSessionSummary) {
    const ago = context.lastSessionTimestamp
      ? formatRelativeTime(context.lastSessionTimestamp)
      : "unknown time";
    parts.push(`\nLast Session (${ago}):\n${context.lastSessionSummary}`);
  }

  if (context.relevantMemory.length > 0) {
    parts.push(`\nRelevant Memory:\n${context.relevantMemory.map((m) => `- ${m}`).join("\n")}`);
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

// ─── Re-export for convenience ────────────────────────────────────────────────

export type { LlmRouterConfig };
export { buildRouterConfig };
