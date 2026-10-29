import { describe, it, expect, vi, beforeEach } from "vitest";

// Live screenshot 2026-10-29: consolidation failed as "LLM gateway (OpenClaw) unreachable". That
// run used compute_mode "gateway" (config.json.bak-20261029T1120-pre-local); the config has been
// "local" since 11:20. In local mode the router only calls Ollama (decide_provider "mode=local"),
// so a failed consolidation or reasoning run must name the local model, never OpenClaw.
const m = vi.hoisted(() => ({
  cfg: {} as Record<string, unknown>,
  routerCalls: [] as Record<string, unknown>[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    const now = Date.now();
    switch (cmd) {
      case "get_config": return m.cfg;
      case "load_terrance_context": return { activeGoals: [], relevantMemory: [] };
      case "list_session_logs":
        return [1, 2, 3].map((i) => ({ id: `s${i}`, timestamp: now - i * 60_000, messageCount: 2, summary: `session ${i}` }));
      case "chat_with_router":
        m.routerCalls.push(args);
        // The real Rust error text from call_ollama when Ollama is down.
        throw "Ollama unreachable — is it running? (error sending request for url (http://127.0.0.1:11434/api/chat))";
      default: return undefined;
    }
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));

import { consolidateRecentSessions, runReasoningLoop } from "./continuity";

describe.each([
  ["compute_mode local", { compute_mode: "local" }],
  ["compute_mode unset (defaults to local)", {}],
])("%s: LLM failures never mention OpenClaw", (_label, cfg) => {
  beforeEach(() => { m.cfg = cfg; m.routerCalls = []; });

  it("consolidation", async () => {
    const r = await consolidateRecentSessions();
    expect(r).toMatchObject({ consolidated: false, error: true, reason: "local model (Ollama) error" });
    expect(r.reason).not.toMatch(/openclaw|gateway/i);
  });

  it("reasoning", async () => {
    const r = await runReasoningLoop();
    expect(r).toMatchObject({ reasoned: false, error: true, reason: "local model (Ollama) error" });
    expect(r.reason).not.toMatch(/openclaw|gateway/i);
  });

  it("routes to local mode with no forced provider", async () => {
    await consolidateRecentSessions();
    await runReasoningLoop();
    expect(m.routerCalls.length).toBeGreaterThanOrEqual(2);
    for (const call of m.routerCalls) {
      expect((call.config as { mode: string }).mode).toBe("local");
      expect(call.forceProvider).toBeUndefined();
    }
  });
});
