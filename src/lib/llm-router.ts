/**
 * LLM Router — TypeScript helpers for cost estimation and provider utilities.
 * Mirrors the pricing tables and token estimation logic in llm_router.rs.
 */

import type { ComputeMode, LlmRouterConfig, CostEstimate, LlmUsageRecord } from "./types";

// ─── Pricing (USD per 1K tokens) ──────────────────────────────────────────────
// Anthropic models — input / output pricing
const ANTHROPIC_PRICING: Record<string, [number, number]> = {
  "claude-haiku-4-5-20251001": [0.00025, 0.00125],
  "claude-sonnet-4-6": [0.003, 0.015],
  "claude-opus-4-6": [0.015, 0.075],
};

// ─── Token estimation ─────────────────────────────────────────────────────────
/** Rough estimate: ~1 token per 4 characters. */
export function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4);
}

// ─── Cost estimation ──────────────────────────────────────────────────────────
export function estimateCostUsd(
  provider: string,
  model: string,
  promptTokens: number,
  completionTokens: number,
): number {
  if (provider !== "anthropic") return 0;
  const pricing = ANTHROPIC_PRICING[model];
  if (!pricing) return 0;
  const [inputRate, outputRate] = pricing;
  return (promptTokens / 1000) * inputRate + (completionTokens / 1000) * outputRate;
}

// ─── Formatting helpers ───────────────────────────────────────────────────────
export function formatCost(usd: number): string {
  if (usd === 0) return "free";
  if (usd < 0.001) return `$${(usd * 1000).toFixed(3)}m`;
  return `$${usd.toFixed(4)}`;
}

export function formatProvider(provider: string): string {
  if (provider === "ollama") return "Local";
  if (provider === "anthropic") return "Anthropic";
  return provider;
}

export function formatLatency(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

// ─── Config builder ───────────────────────────────────────────────────────────
/** Build a RouterConfig from NexusConfig (with sensible defaults). */
export function buildRouterConfig(cfg: {
  compute_mode?: string | null;
  llm_ollama_model?: string | null;
  llm_anthropic_model?: string | null;
  llm_token_threshold?: number | null;
  llm_daily_cap_usd?: number | null;
  llm_per_task_cap_usd?: number | null;
  llm_show_cost_estimate?: boolean | null;
}): LlmRouterConfig {
  return {
    mode: (cfg.compute_mode as ComputeMode) ?? "hybrid",
    ollama_model: cfg.llm_ollama_model ?? "qwen2.5:32b",
    anthropic_model: cfg.llm_anthropic_model ?? "claude-haiku-4-5-20251001",
    token_threshold: cfg.llm_token_threshold ?? 8000,
    daily_cap_usd: cfg.llm_daily_cap_usd ?? 5.0,
    per_task_cap_usd: cfg.llm_per_task_cap_usd ?? 0.10,
    show_cost_estimate: cfg.llm_show_cost_estimate ?? true,
  };
}

// ─── Usage aggregation ────────────────────────────────────────────────────────
export function aggregateUsage(records: LlmUsageRecord[]) {
  const now = Date.now();
  const oneDayMs = 24 * 60 * 60 * 1000;
  const todayRecords = records.filter((r) => now - r.timestamp < oneDayMs);

  const totalCost = records.reduce((s, r) => s + r.estimated_cost_usd, 0);
  const todayCost = todayRecords.reduce((s, r) => s + r.estimated_cost_usd, 0);
  const totalCalls = records.length;
  const todayCalls = todayRecords.length;

  const byProvider = records.reduce(
    (acc, r) => {
      acc[r.provider] = (acc[r.provider] ?? 0) + 1;
      return acc;
    },
    {} as Record<string, number>,
  );

  return { totalCost, todayCost, totalCalls, todayCalls, byProvider };
}
