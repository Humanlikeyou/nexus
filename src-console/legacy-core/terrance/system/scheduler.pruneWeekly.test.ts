import { describe, it, expect, vi, afterEach } from "vitest";

// 7-day simulation of the weekly prune cadence with healthy jobs (tests only; scheduler untouched).
// Context: (2026-10-29): lastPruneTimestamp stuck at 2026-07-06. runReasoningLoop swallowed its
// errors and returned { reasoned: false }, which the job logged as a successful "Skipped"
// without advancing lastReasoningTimestamp. Reasoning stayed due on every tick, took the
// single heavy slot (no backoff after a "success"), and the weekly prune never ran.
// Healthy job mocks advance their own timestamps, like the real implementations.
const stamp = <T,>(key: string, result: T): T => { m.ctx = { ...m.ctx, [key]: Date.now() }; return result; };
const m = vi.hoisted(() => ({
  ctx: {} as Record<string, unknown>,
  reasoning: vi.fn(async () => stamp("lastReasoningTimestamp", { reasoned: true, reflection: "r" })),
  prune: vi.fn(async () => ({ archived: 0, pruned: 0, consolidatedSummary: null, activityArchived: 0 })),
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({
  loadContext: vi.fn(async () => m.ctx), patchContext: vi.fn(async (p: Record<string, unknown>) => { m.ctx = { ...m.ctx, ...p }; return true; }),
  consolidateRecentSessions: vi.fn(async () => stamp("lastConsolidationTimestamp", {})), runReasoningLoop: m.reasoning, generateDailySummary: vi.fn(async () => stamp("lastDailySummaryTimestamp", { sent: true })),
  generateMiddayCorrection: vi.fn(async () => stamp("lastMiddayTimestamp", { sent: true })), generateEveningIntegration: vi.fn(async () => stamp("lastEveningTimestamp", { sent: true })), pruneMemory: m.prune, memoryWriteFailure: vi.fn(() => null),
}));
vi.mock("./selfDiagnosis", () => ({ runDiagnostics: vi.fn(async () => ({ issues: [] })), autoHeal: vi.fn() }));
vi.mock("./rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("./customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("./skillsEvolution", () => ({ evolveSkills: vi.fn(async () => ({ created: 0, updated: 0 })) }));
vi.mock("./subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("./telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

const H = 60 * 60 * 1000;
const D = 24 * H;

describe("weekly prune cadence over a simulated fortnight", () => {
  afterEach(() => vi.useRealTimers());

  it("prunes once on start when overdue, then about every 7 days, never twice inside a week", async () => {
    const start = new Date(2026, 9, 29, 13, 0, 0).getTime();
    vi.useFakeTimers({ now: start });
    m.ctx = {
      dailySummaryHour: 8, lastDailySummaryTimestamp: start - 5 * H, dailySummarySkippedSlot: new Date(2026, 9, 29, 8).getTime(),
      lastHealthCheckTimestamp: start, lastConsolidationTimestamp: start,
      lastMiddayTimestamp: start, lastEveningTimestamp: start,
      lastReasoningTimestamp: start, lastPruneTimestamp: start - 30 * D,
    };
    const pruneTimes: number[] = [];
    m.prune.mockImplementation(async () => {
      pruneTimes.push(Date.now());
      m.ctx = { ...m.ctx, lastPruneTimestamp: Date.now() };
      return { archived: 0, pruned: 0, consolidatedSummary: null, activityArchived: 0 };
    });
    vi.resetModules();
    const sched = await import("./scheduler");
    sched.startScheduler();
    await vi.advanceTimersByTimeAsync(15 * D);
    sched.stopScheduler();

    expect(pruneTimes.length).toBe(3); // overdue run, ~day 7, ~day 14
    expect(pruneTimes[0] - start).toBeLessThan(15 * 60_000);
    for (let i = 1; i < pruneTimes.length; i++) {
      const gap = pruneTimes[i] - pruneTimes[i - 1];
      expect(gap).toBeGreaterThanOrEqual(7 * D);
      expect(gap).toBeLessThan(7 * D + 2 * H);
    }
    expect(m.bus.fail).not.toHaveBeenCalledWith("t", expect.stringMatching(/^Pruning failed/));
  });
});
