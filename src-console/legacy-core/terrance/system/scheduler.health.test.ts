import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const m = vi.hoisted(() => ({
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
  autoHeal: vi.fn(),
  runDiagnostics: vi.fn(async () => ({ issues: [{ service: "n8n", canAutoHeal: true }] })),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => {
  const now = Date.now();
  return {
    loadContext: vi.fn(async () => ({
      activeGoals: [], relevantMemory: [], lastSessionSummary: null, lastSessionTimestamp: null,
      lastDailySummaryTimestamp: Date.now(), lastMiddayTimestamp: Date.now(), lastEveningTimestamp: Date.now(),
      lastConsolidationTimestamp: Date.now(), lastReasoningTimestamp: Date.now(), lastPruneTimestamp: Date.now(),
      lastHealthCheckTimestamp: now - 2 * 60 * 60_000,
    })),
    patchContext: vi.fn(async () => true),
    consolidateRecentSessions: vi.fn(), runReasoningLoop: vi.fn(), generateDailySummary: vi.fn(),
    generateMiddayCorrection: vi.fn(), generateEveningIntegration: vi.fn(), pruneMemory: vi.fn(),
  };
});
vi.mock("./selfDiagnosis", () => ({ runDiagnostics: m.runDiagnostics, autoHeal: m.autoHeal }));
vi.mock("./rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("./customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("./skillsEvolution", () => ({ evolveSkills: vi.fn() }));
vi.mock("./subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("./telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

let stopScheduler: () => void = () => {};

async function healthDetail(status: string | undefined) {
  m.autoHeal.mockResolvedValue(status ? { status, actions: [] } : undefined);
  m.bus.complete.mockClear(); m.bus.fail.mockClear();
  vi.resetModules();
  const sched = await import("./scheduler");
  stopScheduler = sched.stopScheduler;
  sched.startScheduler();
  await vi.advanceTimersByTimeAsync(30_000);
  stopScheduler();
  const all = [...m.bus.complete.mock.calls, ...m.bus.fail.mock.calls].map((c) => String(c[1]));
  return all.find((d) => d.includes("issue(s)"));
}

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { stopScheduler(); vi.useRealTimers(); });

describe("health job status from auto-heal", () => {
  it.each([
    ["declined", "1 issue(s) declined"],
    ["awaiting", "1 issue(s) awaiting"],
    ["failed", "1 issue(s) failed"],
    ["addressed", "1 issue(s) addressed"],
    ["none", "1 issue(s) awaiting"],
    [undefined, "1 issue(s) awaiting"],
  ])("autoHeal %s -> %s (never 'addressed' unless restarts succeeded)", async (status, expected) => {
    expect(await healthDetail(status as string | undefined)).toBe(expected);
  });
});

describe("scheduler status before the first tick", () => {
  it("reports running as soon as it starts and stopped after stop (live check 2026-10-29: Home said Stopped for 30-60s)", async () => {
    vi.resetModules();
    const sched = await import("./scheduler");
    stopScheduler = sched.stopScheduler;
    expect(sched.getSchedulerStatus().running).toBe(false);
    sched.startScheduler();
    expect(sched.getSchedulerStatus().running).toBe(true);
    sched.stopScheduler();
    expect(sched.getSchedulerStatus().running).toBe(false);
  });
});

// OpenClaw is uninstalled: the Health Check never mentions it, never counts it, never auto-heals it.
describe("health job without OpenClaw", () => {
  async function runHealth(report: Record<string, unknown>) {
    m.runDiagnostics.mockResolvedValueOnce(report as never);
    m.autoHeal.mockReset(); m.autoHeal.mockResolvedValue({ status: "awaiting", actions: [] });
    m.bus.complete.mockClear(); m.bus.fail.mockClear();
    vi.resetModules();
    const sched = await import("./scheduler");
    stopScheduler = sched.stopScheduler;
    sched.startScheduler();
    await vi.advanceTimersByTimeAsync(30_000);
    stopScheduler();
    return {
      done: m.bus.complete.mock.calls.map((c) => String(c[1])),
      failed: m.bus.fail.mock.calls.map((c) => String(c[1])),
    };
  }

  it("no issue: completes OK with plain wording, no OpenClaw text, no auto-heal", async () => {
    const { done, failed } = await runHealth({ issues: [] });
    expect(done).toContain("All systems healthy");
    expect(done.join(" ")).not.toMatch(/openclaw/i);
    expect(failed).toEqual([]);
    expect(m.autoHeal).not.toHaveBeenCalled();
  });

  it("with an n8n issue: the count is that issue only", async () => {
    const n8n = { service: "n8n", status: "down", message: "x", canAutoHeal: true };
    const { done } = await runHealth({ issues: [n8n] });
    expect(done).toContain("1 issue(s) awaiting");
    expect(done.join(" ")).not.toMatch(/openclaw/i);
    expect(m.autoHeal).toHaveBeenCalledTimes(1);
    expect((m.autoHeal.mock.calls[0] as unknown[])[0]).toMatchObject({ issues: [n8n] });
  });
});
