import { describe, it, expect, vi, beforeEach } from "vitest";

const m = vi.hoisted(() => ({
  logs: [] as { timestamp: number; summary: string }[],
  saved: [] as Record<string, unknown>[],
  calls: [] as string[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: { context?: Record<string, unknown> }) => {
    m.calls.push(cmd);
    if (cmd === "list_session_logs") return m.logs;
    if (cmd === "chat_with_router") throw "OpenClaw gateway unreachable: error sending request for url (http://127.0.0.1:18789/v1/chat/completions)";
    if (cmd === "load_terrance_context") return { activeGoals: [], relevantMemory: [], lastConsolidationTimestamp: Date.parse("2026-07-06T04:54:00Z") };
    if (cmd === "read_goals") return [];
    if (cmd === "save_terrance_context") { m.saved.push(args?.context ?? {}); return; }
    throw new Error(`unexpected ${cmd}`);
  }),
}));
vi.mock("../../lib/api", () => ({ getConfig: vi.fn(async () => ({})) }));
vi.mock("../../lib/llm-router", () => ({ buildRouterConfig: vi.fn() }));
vi.mock("./deliberation", () => ({ deliberate: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: { register: vi.fn(), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() } }));

import { consolidateRecentSessions, jobFailureReason } from "./continuity";
import { consolidationOutcome } from "./scheduler";

beforeEach(() => { m.logs = []; m.saved = []; m.calls = []; });

// Live check 2026-10-29: skipped consolidations never recorded a run, so the job re-ran every
// 5-min tick and Home showed it overdue since July.
describe("consolidation skip", () => {
  it("records the attempt (lastConsolidationTimestamp = now) without calling the LLM", async () => {
    m.logs = [{ timestamp: Date.now() - 60_000, summary: "only one" }];
    const before = Date.now();
    const r = await consolidateRecentSessions();
    expect(r).toMatchObject({ consolidated: false, skipped: true });
    expect(m.saved).toHaveLength(1);
    expect(m.saved[0].lastConsolidationTimestamp as number).toBeGreaterThanOrEqual(before);
    expect(m.calls).not.toContain("chat_with_router");
    expect(consolidationOutcome(r)).toEqual({ ok: true, detail: "skipped: not enough sessions" });
  });

  it("other outcomes keep their Activity wording", () => {
    expect(consolidationOutcome({ consolidated: false, summary: null, error: true })).toEqual({ ok: false, detail: "Consolidation failed" });
    expect(consolidationOutcome({ consolidated: true, summary: "s", stored: { ok: true } })).toEqual({ ok: true, detail: "Consolidated recent sessions" });
    expect(consolidationOutcome({ consolidated: true, summary: "s", stored: { ok: false, status: 403 } })).toEqual({ ok: false, detail: "Consolidated locally; Letta write failed (HTTP 403)" });
  });
});

// Live check 2026-10-29: compute mode "gateway" with OpenClaw not installed made consolidation and
// the daily summary fail with no reason in Activity.
describe("failure reasons", () => {
  it("consolidation names an unreachable LLM gateway without echoing the raw error", async () => {
    m.logs = [{ timestamp: Date.now() - 60_000, summary: "a" }, { timestamp: Date.now() - 120_000, summary: "b" }];
    const r = await consolidateRecentSessions();
    expect(r).toMatchObject({ consolidated: false, error: true });
    expect(consolidationOutcome(r)).toEqual({ ok: false, detail: "Consolidation failed: Assistant backend not reachable" });
    expect(JSON.stringify(r)).not.toContain("http");
  });

  it("maps known errors to short reasons", () => {
    expect(jobFailureReason("Outbound sends are disabled for this session (NEXUS_OUTBOUND_DISABLED).")).toBe("delivery paused (NEXUS_OUTBOUND_DISABLED)");
    expect(jobFailureReason(new Error("OpenClaw: bad token abc"))).toBe("Assistant backend error");
    expect(jobFailureReason("Ollama request failed")).toBe("local model (Ollama) error");
    expect(jobFailureReason("something with https://x?token=1")).toBe("generation failed");
  });
});
