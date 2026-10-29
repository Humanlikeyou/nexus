import { describe, it, expect, vi, afterEach } from "vitest";

// Live check 2026-10-29 (NEXUS_OUTBOUND_DISABLED=1): the overdue midday touch generated for ~29 s,
// its send was refused by the kill switch and the job was logged as a failure. Backoff retried it
// 5 min later (another 29 s generation, another refusal), each time taking the single heavy slot,
// so the overdue weekly prune waited ~15 min behind it. A refusal is now "Suppressed": neutral,
// no retry for that slot, like rituals (8041224).
const SUPPRESSED = "delivery paused (NEXUS_OUTBOUND_DISABLED)";
const m = vi.hoisted(() => ({
  ctx: {} as Record<string, unknown>,
  midday: vi.fn(),
  reasoning: vi.fn(),
  prune: vi.fn(),
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), suppress: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({
  loadContext: vi.fn(async () => m.ctx), patchContext: vi.fn(async (p: Record<string, unknown>) => { m.ctx = { ...m.ctx, ...p }; return true; }),
  consolidateRecentSessions: vi.fn(), runReasoningLoop: m.reasoning, generateDailySummary: vi.fn(),
  generateMiddayCorrection: m.midday, generateEveningIntegration: vi.fn(), pruneMemory: m.prune, memoryWriteFailure: vi.fn(() => null),
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

describe("kill-switch launch", () => {
  afterEach(() => vi.useRealTimers());

  it("a refused touch is Suppressed once, not retried, and does not starve the weekly prune", async () => {
    const start = new Date(2026, 9, 29, 15, 57, 54).getTime();
    vi.useFakeTimers({ now: start });
    m.ctx = {
      dailySummaryHour: 7, lastDailySummaryTimestamp: start - 115 * D, dailySummarySkippedSlot: new Date(2026, 9, 29, 7).getTime(),
      lastHealthCheckTimestamp: start, lastConsolidationTimestamp: start, lastEveningTimestamp: start,
      lastMiddayTimestamp: start - 115 * D, lastReasoningTimestamp: start - 115 * D, lastPruneTimestamp: start - 115 * D,
    };
    // Real generation time before the refusal (live: 29.4 s), so it counts against the heavy budget.
    m.midday.mockImplementation(async () => {
      await new Promise((r) => setTimeout(r, 29_000));
      return { sent: false, message: "m", reason: SUPPRESSED };
    });
    m.reasoning.mockImplementation(async () => {
      m.ctx = { ...m.ctx, lastReasoningTimestamp: Date.now() };
      return { reasoned: true, reflection: "r" };
    });
    m.prune.mockImplementation(async () => {
      m.ctx = { ...m.ctx, lastPruneTimestamp: Date.now() };
      return { archived: 0, pruned: 0, consolidatedSummary: null, activityArchived: 16589 };
    });
    vi.resetModules();
    const sched = await import("./scheduler");
    sched.startScheduler();
    await vi.advanceTimersByTimeAsync(11 * 60_000); // first tick + two 5-min ticks
    expect(m.prune).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(60 * 60_000); // past every backoff delay, same midday window
    sched.stopScheduler();

    expect(m.midday).toHaveBeenCalledTimes(1);
    expect(m.bus.suppress).toHaveBeenCalledTimes(1);
    expect(m.bus.suppress).toHaveBeenCalledWith("t", "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)");
    expect(m.bus.fail).not.toHaveBeenCalled();
    expect(m.reasoning).toHaveBeenCalledTimes(1);
    expect(m.prune).toHaveBeenCalledTimes(1);
  });

  it("a real delivery failure still backs off and retries", async () => {
    const { sentOutcome } = await import("./scheduler");
    expect(sentOutcome({ sent: false, reason: "Telegram delivery failed" })).toEqual({ ok: false, detail: "Not sent: Telegram delivery failed" });
    expect(sentOutcome({ sent: false, reason: SUPPRESSED }))
      .toEqual({ ok: false, suppressed: true, detail: "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)" });
    expect(sentOutcome({ sent: true })).toEqual({ ok: true, detail: "Sent via Telegram" });
  });
});
