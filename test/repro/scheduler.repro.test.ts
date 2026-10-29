/**
 * Repros for scheduler bugs (TEST_PLAN #2-#5). Fake timers + mocked deps.
 * TZ is pinned to Asia/Tokyo by `npm run test:unit`.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const c = vi.hoisted(() => ({
  loadContext: vi.fn(),
  consolidateRecentSessions: vi.fn(async () => ({ consolidated: false })),
  runReasoningLoop: vi.fn(async () => ({ reasoned: false })),
  generateDailySummary: vi.fn(async () => ({ sent: true })),
  generateMiddayCorrection: vi.fn(async () => ({ sent: true })),
  generateEveningIntegration: vi.fn(async () => ({ sent: true })),
  pruneMemory: vi.fn(async () => ({ archived: 0, pruned: 0 })),
  patchContext: vi.fn(async () => true),
  jobFailureReason: vi.fn(() => "generation failed"),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn() }));
vi.mock("../../src/terrance/system/continuity", () => c);
vi.mock("../../src/terrance/system/selfDiagnosis", () => ({
  runDiagnostics: vi.fn(async () => ({ issues: [] })),
  autoHeal: vi.fn(),
}));
vi.mock("../../src/terrance/system/rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("../../src/terrance/system/customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("../../src/terrance/system/skillsEvolution", () => ({ evolveSkills: vi.fn(async () => ({ created: 0, updated: 0 })) }));
vi.mock("../../src/terrance/system/subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("../../src/terrance/system/telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("../../src/terrance/system/activityBus", () => ({
  activityBus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));

// Fresh module per test: scheduler keeps module-level `running` state (a hung test would leak).
let startScheduler: () => void;
let stopScheduler: () => void;
let BACKOFF_DELAYS_MS: readonly number[];
let MAX_ATTEMPTS_PER_WINDOW: number;

const H = 60 * 60 * 1000;
/** What the real loadContext returns when the Tauri load fails (continuity.ts:123-130). */
const FALLBACK_CTX = { activeGoals: [], lastSessionSummary: null, lastSessionTimestamp: null, relevantMemory: [] };
const at = (iso: string) => new Date(iso).getTime();

/** Start scheduler and run the first tick (fires after 30 s). */
async function firstTick() {
  startScheduler();
  await vi.advanceTimersByTimeAsync(30_000);
}

/** Context where nothing is due except what a test overrides. */
function freshCtx(now: number, over: Record<string, unknown> = {}) {
  return {
    ...FALLBACK_CTX,
    lastDailySummaryTimestamp: now - H,
    lastMiddayTimestamp: now - H,
    lastEveningTimestamp: now - H,
    lastHealthCheckTimestamp: now,
    lastConsolidationTimestamp: now,
    lastReasoningTimestamp: now,
    lastPruneTimestamp: now,
    ...over,
  };
}

describe("scheduler repros", () => {
  beforeEach(async () => {
    vi.resetModules();
    ({ startScheduler, stopScheduler, BACKOFF_DELAYS_MS, MAX_ATTEMPTS_PER_WINDOW } = await import("../../src/terrance/system/scheduler"));
    vi.useFakeTimers();
    Object.values(c).forEach((f) => (f as any).mockClear());
  });
  afterEach(() => {
    stopScheduler();
    vi.useRealTimers();
  });

  it("bug#2: a failed context load does not fire every job at once", async () => {
    vi.setSystemTime(at("2026-10-21T13:00:00+09:00"));
    c.loadContext.mockResolvedValue(FALLBACK_CTX);
    await firstTick();
    const fired = [
      c.generateDailySummary, c.generateMiddayCorrection, c.consolidateRecentSessions,
      c.runReasoningLoop, c.pruneMemory,
    ].filter((f) => f.mock.calls.length > 0).length;
    // Intended: an unreadable context should skip (or at most run health check), not fire 5 jobs.
    expect(fired).toBe(0);
  });

  it("bug#3: failed daily summary follows the 5/15/60-min backoff, max 3 tries per window", async () => {
    const now = at("2026-10-21T07:05:00+09:00");
    vi.setSystemTime(now);
    c.loadContext.mockResolvedValue(freshCtx(now, { lastDailySummaryTimestamp: now - 24 * H }));
    const callTimes: number[] = [];
    c.generateDailySummary.mockImplementation(async () => {
      callTimes.push(Date.now());
      return { sent: false } as any;
    });
    await firstTick();
    // Run through the rest of the daily window and past it. Since 2026-10-29 the daily
    // summary only fires within 30 min of its 07:00 slot, so a 07:05 start fits two tries
    // (07:05:30, then +5 min; the +15 min retry would land after 07:30).
    await vi.advanceTimersByTimeAsync(6 * H);

    expect(MAX_ATTEMPTS_PER_WINDOW).toBe(3);
    expect(BACKOFF_DELAYS_MS).toEqual([5 * 60_000, 15 * 60_000, 60 * 60_000]);
    expect(callTimes.length).toBe(2);
    const slot = at("2026-10-21T07:00:00+09:00");
    for (const t of callTimes) expect(t - slot).toBeLessThan(30 * 60_000);
    expect(callTimes[0]).toBe(now + 30_000); // first tick
    const CHECK = 5 * 60_000;                 // tick granularity
    for (let i = 1; i < callTimes.length; i++) {
      const gap = callTimes[i] - callTimes[i - 1];
      expect(gap, `retry ${i} gap`).toBeGreaterThanOrEqual(BACKOFF_DELAYS_MS[i - 1]);
      expect(gap, `retry ${i} gap`).toBeLessThan(BACKOFF_DELAYS_MS[i - 1] + CHECK);
    }
  });

  it("bug#3b: the next day's window resets the try budget", async () => {
    const now = at("2026-10-21T07:05:00+09:00");
    vi.setSystemTime(now);
    c.loadContext.mockResolvedValue(freshCtx(now, { lastDailySummaryTimestamp: now - 24 * H }));
    c.generateDailySummary.mockResolvedValue({ sent: false } as any);
    await firstTick();
    await vi.advanceTimersByTimeAsync(30 * H); // through 2026-10-22 13:05 (day-2 window closed)
    // Day 1 (07:05 start): 2 tries inside the 30-min window; day 2 gets a fresh budget of 3.
    expect(c.generateDailySummary.mock.calls.length).toBe(2 + MAX_ATTEMPTS_PER_WINDOW);
  });

  it("interval job that fails 3 times is retried after a later interval (not blocked until restart)", async () => {
    const now = at("2026-10-21T14:00:00+09:00");
    vi.setSystemTime(now);
    const consolidationLast = now - 7 * H;
    c.loadContext.mockImplementation(async () => freshCtx(Date.now(), { lastConsolidationTimestamp: consolidationLast }));
    c.consolidateRecentSessions.mockRejectedValue(new Error("llm down"));
    await firstTick();
    await vi.advanceTimersByTimeAsync(2 * H); // exhaust the 3 tries
    expect(c.consolidateRecentSessions).toHaveBeenCalledTimes(3);
    c.consolidateRecentSessions.mockResolvedValue({ consolidated: true } as any);
    await vi.advanceTimersByTimeAsync(24 * H); // a full day later, still overdue
    expect(c.consolidateRecentSessions.mock.calls.length).toBeGreaterThan(3);
  });

  it("bug#4: a hung task does not stall the scheduler forever", async () => {
    const now = at("2026-10-21T07:05:00+09:00");
    vi.setSystemTime(now);
    c.loadContext.mockResolvedValue(freshCtx(now, { lastDailySummaryTimestamp: now - 24 * H }));
    c.generateDailySummary.mockImplementation(() => new Promise(() => {})); // never resolves
    await firstTick();
    const loadsAfterFirst = c.loadContext.mock.calls.length;
    await vi.advanceTimersByTimeAsync(60 * 60_000); // 12 more interval ticks
    // Intended: a per-task timeout lets later ticks proceed.
    expect(c.loadContext.mock.calls.length).toBeGreaterThan(loadsAfterFirst);
  });

  it("bug#5a: midday correction should not fire before 12:30", async () => {
    const now = at("2026-10-21T12:05:00+09:00");
    vi.setSystemTime(now);
    c.loadContext.mockResolvedValue(freshCtx(now, { lastMiddayTimestamp: now - 24 * H }));
    await firstTick();
    // Comment at scheduler.ts:41 says 12:30 PM.
    expect(c.generateMiddayCorrection).not.toHaveBeenCalled();
  });

  it("bug#5b: an evening integration missed before midnight is caught up after midnight", async () => {
    const now = at("2026-10-22T00:30:00+09:00");
    vi.setSystemTime(now);
    // Last evening ran ~27.5h ago, so yesterday's 21:00 run was missed.
    c.loadContext.mockResolvedValue(
      freshCtx(now, { lastEveningTimestamp: now - 27.5 * H, lastDailySummaryTimestamp: now - H }),
    );
    await firstTick();
    expect(c.generateEveningIntegration).toHaveBeenCalledTimes(1);
  });

});
