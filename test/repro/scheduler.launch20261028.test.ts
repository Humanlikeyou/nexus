/**
 * Replay of the 2026-10-28 15:54 JST launch. The fixture holds only the
 * scheduler timestamps from terrance_context.json (lastHealthCheck is set to
 * its pre-launch July value; the launch overwrote the real one).
 * TZ is pinned to Asia/Tokyo by `npm run test:unit`.
 *
 * Root cause: Midday Correction was due, failed in ~25 ms (model missing /
 * Telegram unconfigured) and still spent the tick's single heavy-job budget,
 * so overdue consolidation and reasoning waited for a second tick that never
 * came (the app was closed within 5 min).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import fixture from "./fixtures/terrance_context.2026-10-28.json";

const c = vi.hoisted(() => ({
  loadContext: vi.fn(),
  consolidateRecentSessions: vi.fn(async () => ({ consolidated: false })),
  runReasoningLoop: vi.fn(async () => ({ reasoned: false })),
  generateDailySummary: vi.fn(async () => ({ sent: true })),
  generateMiddayCorrection: vi.fn(async () => ({ sent: true })),
  generateEveningIntegration: vi.fn(async () => ({ sent: true })),
  pruneMemory: vi.fn(async () => ({ archived: 0, pruned: 0 })),
  patchContext: vi.fn(async () => true),
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

let scheduler: typeof import("../../src/terrance/system/scheduler");
const LAUNCH = new Date("2026-10-28T15:54:11+09:00").getTime();
/** Mutable stand-in for terrance_context.json: jobs that succeed persist their timestamp, like the real ones. */
let store: Record<string, unknown>;

describe("2026-10-28 15:54 launch replay", () => {
  beforeEach(async () => {
    vi.resetModules();
    scheduler = await import("../../src/terrance/system/scheduler");
    vi.useFakeTimers();
    vi.setSystemTime(LAUNCH);
    Object.values(c).forEach((f) => (f as any).mockClear());
    store = { activeGoals: [], lastSessionSummary: null, lastSessionTimestamp: null, relevantMemory: [], ...fixture };
    c.loadContext.mockImplementation(async () => ({ ...store }));
    c.consolidateRecentSessions.mockImplementation(async () => {
      store.lastConsolidationTimestamp = Date.now();
      return { consolidated: true } as any;
    });
    c.runReasoningLoop.mockImplementation(async () => {
      store.lastReasoningTimestamp = Date.now();
      return { reasoned: true } as any;
    });
    c.generateMiddayCorrection.mockImplementation(async () => ({ sent: false }) as any);
  });
  afterEach(() => {
    scheduler.stopScheduler();
    vi.useRealTimers();
  });

  it("decideDueTasks: midday, health, consolidation, reasoning and prune are due; daily and evening are outside their windows", () => {
    const ids = scheduler.decideDueTasks(fixture as any, LAUNCH + 30_000).map((t) => t.id);
    expect(ids).toEqual(["midday", "health", "consolidation", "reasoning", "prune"]);
  });

  it("a fast-failing midday does not starve overdue consolidation on the first tick", async () => {
    scheduler.startScheduler();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(c.generateMiddayCorrection).toHaveBeenCalledTimes(1);
    expect(c.consolidateRecentSessions).toHaveBeenCalledTimes(1);
    expect(c.generateDailySummary).not.toHaveBeenCalled();
    // One heavy job that did real work per tick: reasoning waits for the next tick.
    expect(c.runReasoningLoop).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(c.runReasoningLoop).toHaveBeenCalledTimes(1);
  });

  it("a slow heavy failure still spends the budget (no tick of back-to-back LLM timeouts)", async () => {
    c.generateMiddayCorrection.mockImplementation(async () => {
      await new Promise((r) => setTimeout(r, scheduler.FAST_FAIL_MS + 1_000));
      return { sent: false } as any;
    });
    scheduler.startScheduler();
    await vi.advanceTimersByTimeAsync(30_000 + scheduler.FAST_FAIL_MS + 1_000);
    expect(c.generateMiddayCorrection).toHaveBeenCalledTimes(1);
    expect(c.consolidateRecentSessions).not.toHaveBeenCalled();
  });

  it("the failing midday backs off 5/15 min and stops at 3 tries instead of retrying every tick", async () => {
    const times: number[] = [];
    c.generateMiddayCorrection.mockImplementation(async () => {
      times.push(Date.now());
      return { sent: false } as any;
    });
    scheduler.startScheduler();
    await vi.advanceTimersByTimeAsync(4 * 60 * 60_000);
    expect(times.length).toBe(scheduler.MAX_ATTEMPTS_PER_WINDOW);
    for (let i = 1; i < times.length; i++) {
      expect(times[i] - times[i - 1]).toBeGreaterThanOrEqual(scheduler.BACKOFF_DELAYS_MS[i - 1]);
    }
  });
});
