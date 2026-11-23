/**
 * Context Window Compressor
 *
 * Prevents long conversations from degrading by compressing
 * the middle of the conversation history while protecting:
 *  - Head: system prompt + first exchange (sets identity)
 *  - Tail: most recent messages (current context)
 *
 * Inspired by Hermes Agent's compressor. Restored from archive/terrance-legacy (the archive
 * copy is kept) to run on Amara chat histories before they are sent to Hermes. Summaries are
 * made through the Amara Router (it picks the tier; chunks stay under its deep threshold) and
 * pass the injection guard before reaching a prompt.
 *
 * Algorithm:
 *  1. Estimate token count of all messages
 *  2. If under threshold, return messages unchanged
 *  3. Prune old tool outputs (cheap, no LLM call)
 *  4. Protect head (first 3 messages) and tail (last 6 messages)
 *  5. Summarize the middle via LLM into structured format
 *  6. On subsequent compressions, iteratively update the summary with only the messages that
 *     left the tail since the last one (no repeat LLM call when nothing new left the tail)
 */

import { invokeBound } from "../../../bridge";
import { getConfig } from "../../lib/api";
import { buildRouterConfig } from "../../lib/llm-router";
import type { RouterResponse } from "../../lib/types";
import { activityBus } from "./activityBus";
import { scanForInjection } from "./injectionGuard";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface CompressibleMessage {
  role: "user" | "terrance" | "system";
  text: string;
  ts: number;
}

interface CompressionResult {
  messages: CompressibleMessage[];
  compressed: boolean;
  removedCount: number;
  summaryTokens: number;
}

// ─── Constants ──────────────────────────────────────────────────────────────

/** Rough chars per token — conservative estimate */
const CHARS_PER_TOKEN = 4;

/** Start compressing when estimated tokens exceed this */
const COMPRESSION_THRESHOLD_TOKENS = 6000;

/** Protect the first N messages (system prompt + initial exchange) */
const PROTECT_HEAD = 3;

/** Protect the last N messages (recent context) */
const PROTECT_TAIL = 6;

/** Summary prefix injected into compressed conversations */
const SUMMARY_PREFIX =
  "[CONTEXT COMPRESSED] Earlier messages were summarized to preserve context window. " +
  "The summary below covers completed work. Use it and current messages to continue:";

/**
 * Max transcript characters per summarization call. The Amara Router sends anything over its
 * token_threshold (2000 tokens ≈ 8000 chars) to the deep model; summaries are routine, so long
 * middles are summarized in chunks below that, each one updating the running summary.
 */
const SUMMARY_CHUNK_CHARS = 5_000;

/** Pruned tool output placeholder */
const PRUNED_PLACEHOLDER = "[Output trimmed to save context]";

// ─── Token Estimation ───────────────────────────────────────────────────────

function estimateTokens(text: string): number {
  return Math.ceil(text.length / CHARS_PER_TOKEN);
}

function estimateMessagesTokens(messages: CompressibleMessage[]): number {
  return messages.reduce((sum, m) => sum + estimateTokens(m.text) + 4, 0);
}

// ─── Compression ────────────────────────────────────────────────────────────

/** Stored summary from previous compressions for iterative updates */
let _previousSummary: string | null = null;
let _compressionCount = 0;
/** How many leading middle messages `_previousSummary` already covers. */
let _summarizedCount = 0;

/**
 * Compress a conversation if it exceeds the token threshold.
 * Returns the compressed message array, or the original if no compression needed.
 */
export async function compressIfNeeded(
  messages: CompressibleMessage[],
): Promise<CompressionResult> {
  const totalTokens = estimateMessagesTokens(messages);

  if (totalTokens < COMPRESSION_THRESHOLD_TOKENS || messages.length < PROTECT_HEAD + PROTECT_TAIL + 2) {
    return { messages, compressed: false, removedCount: 0, summaryTokens: 0 };
  }

  // Step 1: Prune old tool-like outputs (long messages in the middle)
  const pruned = pruneVerboseMiddle(messages);

  // Re-check after pruning
  const prunedTokens = estimateMessagesTokens(pruned);
  if (prunedTokens < COMPRESSION_THRESHOLD_TOKENS) {
    return { messages: pruned, compressed: false, removedCount: 0, summaryTokens: 0 };
  }

  // Step 2: Split into head / middle / tail
  const head = pruned.slice(0, PROTECT_HEAD);
  const tail = pruned.slice(-PROTECT_TAIL);
  const middle = pruned.slice(PROTECT_HEAD, pruned.length - PROTECT_TAIL);

  if (middle.length === 0) {
    return { messages: pruned, compressed: false, removedCount: 0, summaryTokens: 0 };
  }

  // Step 3: Summarize the middle via LLM (only what the previous summary doesn't cover yet)
  let summary = _previousSummary;
  const fresh = _previousSummary && _summarizedCount <= middle.length ? middle.slice(_summarizedCount) : middle;
  if (!summary || fresh.length > 0) {
    const taskId = activityBus.register("Context Compression", "compression", {
      detail: `${fresh.length} messages → summary`,
    });
    const before = { summary: _previousSummary, count: _summarizedCount };
    if (fresh === middle) _previousSummary = null;
    let next: string | null = null;
    for (const chunk of chunkByChars(fresh, SUMMARY_CHUNK_CHARS)) {
      next = await summarizeMiddle(chunk);
      if (!next) break;
      _previousSummary = next;
    }
    if (!next) {
      _previousSummary = before.summary;
      _summarizedCount = before.count;
      activityBus.fail(taskId, "Summarization failed");
      return { messages: pruned, compressed: false, removedCount: 0, summaryTokens: 0 };
    }
    summary = next;
    _summarizedCount = middle.length;
    _compressionCount++;
    activityBus.complete(taskId, `${messages.length}→${PROTECT_HEAD + 1 + PROTECT_TAIL} messages`);
  }
  if (!summary) return { messages: pruned, compressed: false, removedCount: 0, summaryTokens: 0 };

  // Step 4: Reconstruct messages with summary replacing middle
  const summaryMessage: CompressibleMessage = {
    role: "system",
    text: `${SUMMARY_PREFIX}\n\n${summary}`,
    ts: Date.now(),
  };

  const compressed = [...head, summaryMessage, ...tail];
  const summaryTokens = estimateTokens(summary);

  console.log(
    `[context-compressor] Compressed: ${messages.length}→${compressed.length} messages, ` +
    `${totalTokens}→${estimateMessagesTokens(compressed)} est. tokens (compression #${_compressionCount})`,
  );

  return {
    messages: compressed,
    compressed: true,
    removedCount: middle.length,
    summaryTokens,
  };
}

/**
 * Reset compressor state (call when starting a new session).
 */
export function resetCompressor(): void {
  _previousSummary = null;
  _compressionCount = 0;
  _summarizedCount = 0;
}

/**
 * Get current compressor status for UI display.
 */
export function getCompressorStatus() {
  return {
    compressionCount: _compressionCount,
    hasPreviousSummary: _previousSummary !== null,
  };
}

// ─── Internal: Split a transcript into summarization-sized chunks ───────────

function chunkByChars(messages: CompressibleMessage[], maxChars: number): CompressibleMessage[][] {
  const chunks: CompressibleMessage[][] = [];
  let current: CompressibleMessage[] = [];
  let size = 0;
  for (const m of messages) {
    if (current.length > 0 && size + m.text.length > maxChars) {
      chunks.push(current);
      current = [];
      size = 0;
    }
    current.push(m);
    size += m.text.length;
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

// ─── Internal: Prune verbose middle messages ────────────────────────────────

function pruneVerboseMiddle(messages: CompressibleMessage[]): CompressibleMessage[] {
  return messages.map((m, i) => {
    // Don't touch head or tail
    if (i < PROTECT_HEAD || i >= messages.length - PROTECT_TAIL) return m;
    // Prune overly long messages (>2000 chars) in the middle
    if (m.text.length > 2000) {
      return {
        ...m,
        text: m.text.slice(0, 500) + `\n\n${PRUNED_PLACEHOLDER}\n\n` + m.text.slice(-300),
      };
    }
    return m;
  });
}

// ─── Internal: Summarize middle messages via LLM ────────────────────────────

async function summarizeMiddle(middle: CompressibleMessage[]): Promise<string | null> {
  const transcript = middle
    .map((m) => `${m.role === "user" ? "User" : m.role === "system" ? "System" : "Amara"}: ${m.text}`)
    .join("\n\n");

  const iterativeNote = _previousSummary
    ? `\n\nPrevious compression summary (update and merge, don't discard):\n${_previousSummary}`
    : "";

  const prompt =
    `Summarize the following conversation segment into a structured summary. ` +
    `This summary will replace these messages in the context window.\n\n` +
    `Format your summary as:\n` +
    `**Goal**: What the user is working toward\n` +
    `**Progress**: What was accomplished in these messages\n` +
    `**Decisions**: Key choices or directions taken\n` +
    `**Context**: Important details for continuing (file names, values, states)\n` +
    `**Next Steps**: What was about to happen when these messages ended\n\n` +
    `Be concise but preserve all actionable information. ` +
    `Do not lose file paths, variable names, or specific values mentioned.` +
    `${iterativeNote}\n\n---\n\n${transcript}`;

  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    // The router decides the tier; chunking keeps routine summaries under its deep threshold.
    const resp = await invokeBound<RouterResponse>("chat_with_router", {
      text: prompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
    });

    // The summary is model output built from earlier content: guard it before it is re-sent.
    return scanForInjection(resp.content.trim()).sanitized.trim() || null;
  } catch (e) {
    console.error("[context-compressor] Summarization failed:", e);
    return null;
  }
}
