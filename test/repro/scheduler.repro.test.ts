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
    ({ startScheduler, stopScheduler } = await import("../../src/terrance/system/scheduler"));
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

  it("bug#3: failed daily summary is not retried every 5 minutes", async () => {
    const now = at("2026-10-21T07:05:00+09:00");
    vi.setSystemTime(now);
    c.loadContext.mockResolvedValue(freshCtx(now, { lastDailySummaryTimestamp: now - 24 * H }));
    c.generateDailySummary.mockResolvedValue({ sent: false } as any);
    await firstTick();
    await vi.advanceTimersByTimeAsync(5 * 60_000 * 3); // 3 more ticks
    // Intended: backoff → at most one attempt within 20 minutes.
    expect(c.generateDailySummary.mock.calls.length).toBeLessThanOrEqual(1);
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

  it.todo(
    "decideDueTasks(ctx, now) pure API: table-driven window tests — blocked on approved refactor extracting decision logic from tick()",
  );
});
