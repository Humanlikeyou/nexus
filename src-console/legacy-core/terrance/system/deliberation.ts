/**
 * Adversarial Deliberation System
 *
 * Implements a bull/bear debate pattern for important decisions.
 * Two LLM passes with opposing system prompts (optimistic vs skeptical),
 * then a synthesis pass that weighs both perspectives.
 *
 * Inspired by TradingAgents' multi-agent debate architecture.
 *
 * Usage:
 *  - Called from the reasoning loop for goal prioritization
 *  - Called from daily summary for suggested priorities
 *  - Triggered from Amara chat via "/deliberate <topic>" (deep: true → the router's deep tier)
 */

import { invokeBound } from "../../../bridge";
import { getConfig } from "../../lib/api";
import { buildRouterConfig } from "../../lib/llm-router";
import type { RouterResponse } from "../../lib/types";
import { activityBus } from "./activityBus";
import { scanForInjection } from "./injectionGuard";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface DeliberationResult {
  bullCase: string;
  bearCase: string;
  synthesis: string;
  confidence: "high" | "medium" | "low";
  timestamp: number;
  /** Tier the router reported for the synthesis pass (its reply metadata). */
  tier?: RouterResponse["tier"];
  routingReason?: string;
}

export interface DeliberateOptions {
  /** Ask the router for its deep tier on every pass (X-Amara-Route: deep). */
  deep?: boolean;
  /** Receives the router's user-facing error text when a pass fails. */
  onError?: (message: string) => void;
}

// ─── System Prompts ─────────────────────────────────────────────────────────

const BULL_PROMPT =
  `You are an optimistic strategist. Analyze the topic with a bias toward opportunity, ` +
  `momentum, and upside potential. Identify what's working, what could accelerate, ` +
  `and why the positive scenario is more likely. Be specific and evidence-based, ` +
  `not blindly optimistic. Keep your analysis to 5-8 bullet points.`;

const BEAR_PROMPT =
  `You are a skeptical risk analyst. Analyze the topic with a bias toward risk, ` +
  `friction, and downside scenarios. Identify what could go wrong, what's being ` +
  `overlooked, and why caution is warranted. Be specific and evidence-based, ` +
  `not blindly pessimistic. Keep your analysis to 5-8 bullet points.`;

const SYNTHESIS_PROMPT =
  `You are Amara, synthesizing two opposing analyses into a balanced assessment. ` +
  `You have a bull case (optimistic) and a bear case (skeptical). ` +
  `Weigh both perspectives and produce:\n` +
  `1. **Verdict**: The most likely outcome given both views\n` +
  `2. **Action items**: 3-5 concrete next steps\n` +
  `3. **Watch points**: 2-3 signals that would change your assessment\n` +
  `4. **Confidence**: high/medium/low based on how much the two views agree\n\n` +
  `Be concise and actionable.`;

// ─── Core ───────────────────────────────────────────────────────────────────

/**
 * Run a full deliberation cycle on a topic.
 * Three sequential LLM calls: bull → bear → synthesis.
 */
export async function deliberate(
  topic: string,
  context?: string,
  options: DeliberateOptions = {},
): Promise<DeliberationResult | null> {
  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;
    let last: RouterResponse | null = null;

    const callLLM = async (prompt: string): Promise<string> => {
      const resp = await invokeBound<RouterResponse>("chat_with_router", {
        text: prompt,
        config: routerConfig,
        tags: [],
        forceProvider: undefined,
        anthropicApiKey: apiKey,
        deep: options.deep ?? false,
      });
      last = resp;
      return resp.content.trim();
    };
    // Bull/bear output is re-fed into the synthesis prompt: guard it first.
    const guarded = (text: string) => scanForInjection(text).sanitized.trim();

    const contextBlock = context ? `\n\nAdditional context:\n${context}` : "";
    const taskId = activityBus.register(`Deliberation: ${topic.slice(0, 40)}`, "deliberation");

    // Pass 1: Bull case
    activityBus.updateSteps(taskId, [
      { label: "Bull Case", status: "running" },
      { label: "Bear Case", status: "pending" },
      { label: "Synthesis", status: "pending" },
    ]);
    const bullCase = await callLLM(
      `${BULL_PROMPT}\n\nTopic: ${topic}${contextBlock}`,
    );

    // Pass 2: Bear case
    activityBus.updateSteps(taskId, [
      { label: "Bull Case", status: "complete" },
      { label: "Bear Case", status: "running" },
      { label: "Synthesis", status: "pending" },
    ]);
    const bearCase = await callLLM(
      `${BEAR_PROMPT}\n\nTopic: ${topic}${contextBlock}`,
    );

    // Pass 3: Synthesis
    activityBus.updateSteps(taskId, [
      { label: "Bull Case", status: "complete" },
      { label: "Bear Case", status: "complete" },
      { label: "Synthesis", status: "running" },
    ]);
    const synthesis = await callLLM(
      `${SYNTHESIS_PROMPT}\n\n` +
      `Topic: ${topic}\n\n` +
      `Bull Case (optimistic view):\n${guarded(bullCase)}\n\n` +
      `Bear Case (skeptical view):\n${guarded(bearCase)}`,
    );
    const synthesisReply = last as RouterResponse | null;

    // Extract confidence from synthesis
    const confidence = extractConfidence(synthesis);
    activityBus.complete(taskId, `Confidence: ${confidence}`);

    return {
      bullCase,
      bearCase,
      synthesis,
      confidence,
      timestamp: Date.now(),
      tier: synthesisReply?.tier,
      routingReason: synthesisReply?.routing_reason,
    };
  } catch (e) {
    console.error("[deliberation] Failed:", e);
    options.onError?.(e instanceof Error ? e.message : String(e));
    return null;
  }
}

/**
 * Lightweight deliberation — only bull + bear, no synthesis.
 * Useful for quick second opinions on less critical decisions.
 */
export async function quickDeliberate(
  topic: string,
): Promise<{ bull: string; bear: string } | null> {
  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const callLLM = async (prompt: string): Promise<string> => {
      const resp = await invokeBound<RouterResponse>("chat_with_router", {
        text: prompt,
        config: routerConfig,
        tags: [],
        forceProvider: undefined,
        anthropicApiKey: apiKey,
      });
      return resp.content.trim();
    };

    const bull = await callLLM(`${BULL_PROMPT}\n\nTopic: ${topic}`);
    const bear = await callLLM(`${BEAR_PROMPT}\n\nTopic: ${topic}`);

    return { bull, bear };
  } catch (e) {
    console.error("[deliberation] Quick deliberation failed:", e);
    return null;
  }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function extractConfidence(synthesis: string): "high" | "medium" | "low" {
  const lower = synthesis.toLowerCase();
  if (lower.includes("confidence: high") || lower.includes("**confidence**: high")) return "high";
  if (lower.includes("confidence: low") || lower.includes("**confidence**: low")) return "low";
  return "medium";
}
