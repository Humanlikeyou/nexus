import { describe, it, expect, vi, afterEach } from "vitest";

const m = vi.hoisted(() => ({
  ctx: {} as Record<string, unknown>,
  daily: vi.fn(async () => ({ sent: true, summary: "s" })),
  patch: vi.fn(async (_p: Record<string, unknown>) => true),
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({
  loadContext: vi.fn(async () => m.ctx), patchContext: m.patch,
  consolidateRecentSessions: vi.fn(), runReasoningLoop: vi.fn(), generateDailySummary: m.daily,
  generateMiddayCorrection: vi.fn(), generateEveningIntegration: vi.fn(), pruneMemory: vi.fn(), memoryWriteFailure: vi.fn(),
}));
vi.mock("./selfDiagnosis", () => ({ runDiagnostics: vi.fn(async () => ({ issues: [] })), autoHeal: vi.fn() }));
vi.mock("./rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("./customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("./skillsEvolution", () => ({ evolveSkills: vi.fn() }));
vi.mock("./subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("./telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

import { decideDueTasks, missedDailySummarySlot, DAILY_SUMMARY_WINDOW_MS } from "./scheduler";

const H = 60 * 60 * 1000;
const today8 = new Date(2026, 9, 29, 8, 0, 0).getTime();
const yesterday = today8 - 24 * H + 5 * 60_000;
const recent = (now: number) => ({
  dailySummaryHour: 8, lastDailySummaryTimestamp: yesterday,
  lastHealthCheckTimestamp: now, lastConsolidationTimestamp: now, lastReasoningTimestamp: now, lastPruneTimestamp: now,
  lastMiddayTimestamp: now, lastEveningTimestamp: now,
});

// Live check 2026-10-29: a launch at 11:10 tried to catch up the 07:00/08:00 daily summary.
describe("daily summary 30-min window", () => {
  it("matches the ritual window", () => expect(DAILY_SUMMARY_WINDOW_MS).toBe(30 * 60_000));

  it("is due within 30 min of its slot", () => {
    const now = today8 + 20 * 60_000;
    expect(decideDueTasks(recent(now), now).map((t) => t.id)).toContain("daily");
    expect(missedDailySummarySlot(recent(now), now)).toBeNull();
  });

  it("2.5h late: not due, reported once as missed", () => {
    const now = today8 + 2.5 * H;
    expect(decideDueTasks(recent(now), now).map((t) => t.id)).not.toContain("daily");
    expect(missedDailySummarySlot(recent(now), now)).toBe(today8);
    expect(missedDailySummarySlot({ ...recent(now), dailySummarySkippedSlot: today8 }, now)).toBeNull();
  });

  it("already sent today or never seeded: nothing to skip", () => {
    const now = today8 + 2.5 * H;
    expect(missedDailySummarySlot({ ...recent(now), lastDailySummaryTimestamp: today8 + 60_000 }, now)).toBeNull();
    expect(missedDailySummarySlot({ dailySummaryHour: 8 }, now)).toBeNull();
  });
});

describe("tick on a 2.5h-late launch", () => {
  afterEach(() => vi.useRealTimers());

  it("records 'Skipped (missed window)' once and never generates or sends", async () => {
    const start = today8 + 2.5 * H - 30_000;
    vi.useFakeTimers({ now: start });
    m.ctx = recent(start + 30_000);
    vi.resetModules();
    const sched = await import("./scheduler");
    sched.startScheduler();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(m.patch).toHaveBeenCalledWith({ dailySummarySkippedSlot: today8 });
    expect(m.bus.register).toHaveBeenCalledWith("Daily Summary", "daily_summary");
    expect(m.bus.complete.mock.calls.map((c) => c[1])).toContain("Skipped (missed window)");
    expect(m.daily).not.toHaveBeenCalled();

    // Next tick with the skip recorded: no second entry.
    m.ctx = { ...m.ctx, dailySummarySkippedSlot: today8 };
    m.patch.mockClear();
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(m.patch).not.toHaveBeenCalledWith({ dailySummarySkippedSlot: today8 });
    expect(m.daily).not.toHaveBeenCalled();
    sched.stopScheduler();
  });
});
