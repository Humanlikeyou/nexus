import { describe, it, expect, vi, afterEach } from "vitest";

// Weekly prune while other heavy jobs keep failing (tests only; scheduler untouched).
// pruneWeekly.test.ts covers healthy jobs; pruneStarvation.test.ts covers 11 minutes of failing
// reasoning. Here reasoning AND consolidation fail for 15 days, midday/evening fail on alternate
// days, and prune's own failure backoff is pinned. Failing mocks take 30 s of (fake) LLM time,
// so they spend the tick's heavy budget (fast failures under FAST_FAIL_MS give it back).
const stamp = <T,>(key: string, result: T): T => { m.ctx = { ...m.ctx, [key]: Date.now() }; return result; };
const slow = (ms = 30_000) => new Promise((r) => setTimeout(r, ms));
const m = vi.hoisted(() => ({
  ctx: {} as Record<string, unknown>,
  runs: [] as { job: string; at: number }[],
  reasoning: vi.fn(),
  consolidate: vi.fn(),
  midday: vi.fn(),
  evening: vi.fn(),
  daily: vi.fn(),
  prune: vi.fn(),
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), suppress: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({
  loadContext: vi.fn(async () => m.ctx), patchContext: vi.fn(async (p: Record<string, unknown>) => { m.ctx = { ...m.ctx, ...p }; return true; }),
  consolidateRecentSessions: m.consolidate, runReasoningLoop: m.reasoning, generateDailySummary: m.daily,
  generateMiddayCorrection: m.midday, generateEveningIntegration: m.evening, pruneMemory: m.prune, memoryWriteFailure: vi.fn(() => null),
  jobFailureReason: vi.fn(() => "generation failed"),
}));
vi.mock("./selfDiagnosis", () => ({ runDiagnostics: vi.fn(async () => ({ issues: [] })), autoHeal: vi.fn() }));
vi.mock("./rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("./customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("./skillsEvolution", () => ({ evolveSkills: vi.fn(async () => ({ created: 0, updated: 0 })) }));
vi.mock("./subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("./telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

const MIN = 60_000;
const H = 60 * MIN;
const D = 24 * H;
const TICK = 5 * MIN;
const start = new Date(2026, 9, 29, 13, 0, 0).getTime();
const record = (job: string) => m.runs.push({ job, at: Date.now() });
// Ticks: the startup tick at start + 30 s, then setInterval at start + 5, 10, 15 … min.
const tickOf = (at: number) => Math.floor((at - start) / TICK);
const ok = { archived: 0, pruned: 0, consolidatedSummary: null, activityArchived: 0 };

function healthyCtx(overrides: Record<string, unknown> = {}) {
  return {
    dailySummaryHour: 8, lastDailySummaryTimestamp: start - 5 * H, dailySummarySkippedSlot: new Date(2026, 9, 29, 8).getTime(),
    lastHealthCheckTimestamp: start, lastConsolidationTimestamp: start, lastReasoningTimestamp: start,
    lastMiddayTimestamp: start, lastEveningTimestamp: start, ...overrides,
  };
}

async function simulate(ms: number) {
  vi.resetModules();
  const sched = await import("./scheduler");
  sched.startScheduler();
  await vi.advanceTimersByTimeAsync(ms);
  sched.stopScheduler();
}

describe("weekly prune under failing heavy jobs", () => {
  afterEach(() => {
    vi.useRealTimers();
    m.runs = [];
    for (const f of [m.reasoning, m.consolidate, m.midday, m.evening, m.daily, m.prune, m.bus.fail, m.bus.complete]) f.mockReset();
  });

  it("15 days of failing reasoning + consolidation (and midday/evening on alternate days): prune keeps its weekly cadence", async () => {
    vi.useFakeTimers({ now: start });
    m.ctx = healthyCtx({ lastConsolidationTimestamp: start - 7 * H, lastReasoningTimestamp: start - 13 * H, lastPruneTimestamp: start - 30 * D });
    const odd = () => Math.floor((Date.now() - start) / D) % 2 === 1;
    m.reasoning.mockImplementation(async () => { record("reasoning"); await slow(); throw new Error("LLM gateway error"); });
    m.consolidate.mockImplementation(async () => { record("consolidation"); await slow(); return { consolidated: false, error: true, reason: "local model (Ollama) error" }; });
    m.midday.mockImplementation(async () => {
      record("midday"); await slow();
      return odd() ? { sent: false, message: null, reason: "Telegram delivery failed" } : stamp("lastMiddayTimestamp", { sent: true, message: "m" });
    });
    m.evening.mockImplementation(async () => {
      record("evening"); await slow();
      return odd() ? stamp("lastEveningTimestamp", { sent: true, message: "e" }) : { sent: false, message: null, reason: "Telegram delivery failed" };
    });
    m.daily.mockImplementation(async () => { record("daily"); await slow(); return stamp("lastDailySummaryTimestamp", { sent: true, summary: "s" }); });
    m.prune.mockImplementation(async () => { record("prune"); await slow(5_000); return stamp("lastPruneTimestamp", ok); });

    await simulate(15 * D);

    const prunes = m.runs.filter((r) => r.job === "prune").map((r) => r.at);
    expect(prunes).toHaveLength(3); // overdue start, ~day 7, ~day 14
    expect(prunes[0] - start).toBeLessThan(H); // overdue: within the first hour despite the failures
    for (let i = 1; i < prunes.length; i++) {
      expect(prunes[i] - prunes[i - 1]).toBeGreaterThanOrEqual(7 * D);
      expect(prunes[i] - prunes[i - 1]).toBeLessThan(7 * D + 2 * H);
    }

    // One heavy job per tick, and no job holds the slot tick after tick.
    const byTick = new Map<number, string[]>();
    for (const r of m.runs) byTick.set(tickOf(r.at), [...(byTick.get(tickOf(r.at)) ?? []), r.job]);
    for (const jobs of byTick.values()) expect(jobs).toHaveLength(1);
    const ticks = [...byTick.keys()].sort((a, b) => a - b);
    let streak = 1;
    let maxStreak = 1;
    for (let i = 1; i < ticks.length; i++) {
      const same = ticks[i] === ticks[i - 1] + 1 && byTick.get(ticks[i])![0] === byTick.get(ticks[i - 1])![0];
      streak = same ? streak + 1 : 1;
      maxStreak = Math.max(maxStreak, streak);
    }
    expect(maxStreak).toBeLessThanOrEqual(2);

    // Failing jobs respect their own cap: ≤ 3 attempts per window (6 h consolidation, 12 h reasoning).
    const perWindow = (job: string, interval: number, last: number) => {
      const counts = new Map<number, number>();
      for (const r of m.runs.filter((x) => x.job === job)) {
        const w = Math.floor((r.at - (last + interval)) / interval);
        counts.set(w, (counts.get(w) ?? 0) + 1);
      }
      return Math.max(...counts.values());
    };
    expect(perWindow("consolidation", 6 * H, start - 7 * H)).toBe(3);
    expect(perWindow("reasoning", 12 * H, start - 13 * H)).toBe(3);
    expect(m.reasoning.mock.calls.length).toBeLessThanOrEqual(3 * 31);
  });

  it("prune failure backoff: retries at +9.5 and +24.5 min (tick-quantised 5/15), 3 attempts per window, then nothing until the window rolls", async () => {
    vi.useFakeTimers({ now: start });
    const lastPrune = start - 8 * D; // due since start − 1 day; window rolls at start + 6 days
    m.ctx = healthyCtx({ lastPruneTimestamp: lastPrune });
    // Every other job is healthy here, so the only failures are prune's.
    m.reasoning.mockImplementation(async () => stamp("lastReasoningTimestamp", { reasoned: true, reflection: "r" }));
    m.consolidate.mockImplementation(async () => stamp("lastConsolidationTimestamp", { consolidated: true, summary: "s" }));
    m.daily.mockImplementation(async () => stamp("lastDailySummaryTimestamp", { sent: true, summary: "s" }));
    m.midday.mockImplementation(async () => stamp("lastMiddayTimestamp", { sent: true, message: "m" }));
    m.evening.mockImplementation(async () => stamp("lastEveningTimestamp", { sent: true, message: "e" }));
    let failing = true;
    m.prune.mockImplementation(async () => {
      record("prune");
      return failing ? { ...ok, error: true, reason: "disk full" } : stamp("lastPruneTimestamp", ok);
    });

    await simulate(6 * D - 2 * MIN);
    const attempts = m.runs.map((r) => r.at - start);
    // Real numbers (scheduler.ts / backoff.ts): BACKOFF_DELAYS_MS 5/15/60 min counted from the
    // failed tick's start, MAX_ATTEMPTS_PER_WINDOW = 3, and retries wait for the next 5-min tick
    // (start + 5k min; the startup tick is at start + 30 s). So: start + 0:30, + 10:00 (5 min
    // after 0:30 is 5:30, next tick 10:00), + 25:00 (10:00 + 15). The 60-min step is never
    // reached: the third failure exhausts the window.
    expect(attempts).toEqual([30_000, 10 * MIN, 25 * MIN]);
    expect(attempts.map((a) => a - attempts[0])).toEqual([0, 9.5 * MIN, 24.5 * MIN]);
    expect(m.ctx.lastPruneTimestamp).toBe(lastPrune);
    expect(m.bus.fail.mock.calls.filter((c) => c[1] === "Pruning failed: disk full")).toHaveLength(3);
    expect(m.bus.fail.mock.calls.every((c) => c[1] === "Pruning failed: disk full")).toBe(true);

    // Window rolls at lastPrune + 14 d (= start + 6 d): the next attempt succeeds; cadence resumes.
    failing = false;
    m.runs = [];
    vi.setSystemTime(start + 6 * D - 2 * MIN);
    m.ctx = { ...m.ctx, ...healthyCtx({ lastPruneTimestamp: lastPrune, lastHealthCheckTimestamp: Date.now(), lastConsolidationTimestamp: Date.now(), lastReasoningTimestamp: Date.now(), lastMiddayTimestamp: Date.now(), lastEveningTimestamp: Date.now() }) };
    await simulate(8 * D);
    const prunes = m.runs.map((r) => r.at);
    expect(prunes).toHaveLength(2);
    expect(prunes[0] - (start + 6 * D)).toBeLessThan(10 * MIN); // first tick after the window rolls
    expect(prunes[1] - prunes[0]).toBeGreaterThanOrEqual(7 * D);
    expect(prunes[1] - prunes[0]).toBeLessThan(7 * D + 2 * H);
  });
});
