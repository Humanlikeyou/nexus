import { describe, it, expect, vi, afterEach } from "vitest";

// Bug (2026-10-29): lastPruneTimestamp stuck at 2026-07-06. runReasoningLoop swallowed its
// errors and returned { reasoned: false }, which the job logged as a successful "Skipped"
// without advancing lastReasoningTimestamp. Reasoning stayed due on every tick, took the
// single heavy slot (no backoff after a "success"), and the weekly prune never ran.
const m = vi.hoisted(() => ({
  ctx: {} as Record<string, unknown>,
  reasoning: vi.fn(async () => ({ reasoned: false, reflection: null, error: true, reason: "LLM gateway error" })),
  prune: vi.fn(async () => ({ archived: 0, pruned: 0, consolidatedSummary: null, activityArchived: 0 })),
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({
  loadContext: vi.fn(async () => m.ctx), patchContext: vi.fn(async () => true),
  consolidateRecentSessions: vi.fn(), runReasoningLoop: m.reasoning, generateDailySummary: vi.fn(),
  generateMiddayCorrection: vi.fn(), generateEveningIntegration: vi.fn(), pruneMemory: m.prune, memoryWriteFailure: vi.fn(() => null),
}));
vi.mock("./selfDiagnosis", () => ({ runDiagnostics: vi.fn(async () => ({ issues: [] })), autoHeal: vi.fn() }));
vi.mock("./rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("./customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("./skillsEvolution", () => ({ evolveSkills: vi.fn() }));
vi.mock("./subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("./telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

const H = 60 * 60 * 1000;
const D = 24 * H;

describe("weekly prune is not starved by a failing reasoning loop", () => {
  afterEach(() => vi.useRealTimers());

  it("a reasoning failure is reported as a failure, not 'Skipped'", async () => {
    const { reasoningOutcome } = await import("./scheduler");
    expect(reasoningOutcome({ reasoned: false, reflection: null, error: true, reason: "LLM gateway error" }))
      .toEqual({ ok: false, detail: "Reasoning failed: LLM gateway error" });
    expect(reasoningOutcome({ reasoned: false, reflection: null }).ok).toBe(false);
    expect(reasoningOutcome({ reasoned: true, reflection: "r" })).toEqual({ ok: true, detail: "Reflection complete" });
  });

  it("prune runs (and its timestamp can advance) while reasoning keeps failing", async () => {
    const start = new Date(2026, 9, 29, 13, 0, 0).getTime();
    vi.useFakeTimers({ now: start });
    m.ctx = {
      dailySummaryHour: 8, lastDailySummaryTimestamp: start - 5 * H, dailySummarySkippedSlot: new Date(2026, 9, 29, 8).getTime(),
      lastHealthCheckTimestamp: start, lastConsolidationTimestamp: start,
      lastMiddayTimestamp: start, lastEveningTimestamp: start,
      lastReasoningTimestamp: start - 115 * D, lastPruneTimestamp: start - 115 * D,
    };
    m.prune.mockImplementation(async () => {
      m.ctx = { ...m.ctx, lastPruneTimestamp: Date.now() };
      return { archived: 0, pruned: 0, consolidatedSummary: null, activityArchived: 0 };
    });
    vi.resetModules();
    const sched = await import("./scheduler");
    sched.startScheduler();
    await vi.advanceTimersByTimeAsync(11 * 60_000); // first tick + two 5-min ticks
    sched.stopScheduler();
    expect(m.reasoning).toHaveBeenCalled();
    expect(m.prune).toHaveBeenCalledTimes(1);
    expect((m.ctx.lastPruneTimestamp as number) > start - D).toBe(true);
    expect(m.bus.fail).toHaveBeenCalledWith("t", "Reasoning failed: LLM gateway error");
  });
});
