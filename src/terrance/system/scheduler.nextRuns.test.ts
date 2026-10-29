import { describe, it, expect, vi, afterEach } from "vitest";

const m = vi.hoisted(() => ({
  consolidate: vi.fn(async () => ({ consolidated: true })),
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({
  loadContext: vi.fn(), patchContext: vi.fn(async () => true),
  consolidateRecentSessions: m.consolidate, runReasoningLoop: vi.fn(), generateDailySummary: vi.fn(),
  generateMiddayCorrection: vi.fn(), generateEveningIntegration: vi.fn(), pruneMemory: vi.fn(),
}));
vi.mock("./selfDiagnosis", () => ({ runDiagnostics: vi.fn(), autoHeal: vi.fn() }));
vi.mock("./rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("./customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("./skillsEvolution", () => ({ evolveSkills: vi.fn() }));
vi.mock("./subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("./telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

import { computeNextRuns, nextSlot, runConsolidationNow, getSchedulerStatus } from "./scheduler";

const H = 60 * 60 * 1000;

describe("computeNextRuns", () => {
  const now = new Date(2026, 9, 28, 14, 0, 0).getTime(); // local 14:00

  it("adds the 6h / 12h intervals to the last runs", () => {
    const r = computeNextRuns({ dailySummaryHour: 8, lastConsolidationTimestamp: now - H, lastReasoningTimestamp: now - 2 * H, lastDailySummaryTimestamp: now - H }, now);
    expect(r.nextConsolidationAt).toBe(now + 5 * H);
    expect(r.nextReasoningAt).toBe(now + 10 * H);
  });

  it("daily summary: tomorrow's slot once today's has run, today's slot if not", () => {
    const today8 = new Date(2026, 9, 28, 8, 0, 0).getTime();
    const tomorrow8 = new Date(2026, 9, 29, 8, 0, 0).getTime();
    expect(computeNextRuns({ dailySummaryHour: 8, lastDailySummaryTimestamp: today8 + 1000 }, now).nextDailySummaryAt).toBe(tomorrow8);
    // 14:00 is past today's 30-min window: it is skipped, so the next one is tomorrow.
    expect(computeNextRuns({ dailySummaryHour: 8, lastDailySummaryTimestamp: today8 - 2 * H }, now).nextDailySummaryAt).toBe(tomorrow8);
    expect(computeNextRuns({ dailySummaryHour: 8, lastDailySummaryTimestamp: today8 - 2 * H }, today8 + 10 * 60_000).nextDailySummaryAt).toBe(today8);
  });

  it("unseeded interval jobs have no next time", () => {
    const r = computeNextRuns({ dailySummaryHour: 8 }, now);
    expect(r.nextConsolidationAt).toBeNull();
    expect(r.nextReasoningAt).toBeNull();
    expect(r.nextDailySummaryAt).toBe(nextSlot(now, 8));
  });

  it("initial status has null next times", () => {
    const s = getSchedulerStatus();
    expect(s.nextConsolidationAt).toBeNull();
    expect(s.nextDailySummaryAt).toBeNull();
  });
});

describe("runConsolidationNow", () => {
  afterEach(() => { m.consolidate.mockReset(); m.consolidate.mockImplementation(async () => ({ consolidated: true })); });

  it("runs consolidation and logs it to the activity bus", async () => {
    await expect(runConsolidationNow()).resolves.toEqual({ started: true });
    expect(m.consolidate).toHaveBeenCalledTimes(1);
    expect(m.bus.complete).toHaveBeenCalledWith("t", "Consolidated recent sessions");
  });

  it("is single-flight: a second call while one runs reports busy", async () => {
    let release!: () => void;
    m.consolidate.mockImplementation(() => new Promise((r) => { release = () => r({ consolidated: false }); }));
    const first = runConsolidationNow();
    await expect(runConsolidationNow()).resolves.toEqual({ started: false, reason: "busy" });
    release();
    await expect(first).resolves.toEqual({ started: true });
    expect(m.consolidate).toHaveBeenCalledTimes(1);
    m.consolidate.mockImplementation(async () => ({ consolidated: true }));
    await expect(runConsolidationNow()).resolves.toEqual({ started: true }); // guard released
  });
});
