import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";

// The on-time daily summary, end to end through tick(): a slot inside the 30-min window runs
// exactly once, later ticks and a relaunch in the same window don't re-run it, a day it never
// ran is skipped once, and the next day's slot runs again. Context goes through a JSON round
// trip on every load, like terrance_context.json.
const SUPPRESSED = "delivery paused (NEXUS_OUTBOUND_DISABLED)";
const m = vi.hoisted(() => ({
  store: "{}",
  sendsDisabled: false,
  daily: vi.fn(),
  bus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), suppress: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));
const persisted = () => JSON.parse(m.store) as Record<string, unknown>;
const persist = (p: Record<string, unknown>) => { m.store = JSON.stringify({ ...persisted(), ...p }); };
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({
  loadContext: vi.fn(async () => persisted()), patchContext: vi.fn(async (p: Record<string, unknown>) => { persist(p); return true; }),
  consolidateRecentSessions: vi.fn(), runReasoningLoop: vi.fn(), generateDailySummary: m.daily,
  generateMiddayCorrection: vi.fn(), generateEveningIntegration: vi.fn(), pruneMemory: vi.fn(), memoryWriteFailure: vi.fn(() => null),
}));
vi.mock("./selfDiagnosis", () => ({ runDiagnostics: vi.fn(async () => ({ issues: [] })), autoHeal: vi.fn() }));
vi.mock("./rituals", () => ({ tickRituals: vi.fn() }));
vi.mock("./customJobs", () => ({ tickCustomJobs: vi.fn(async () => []) }));
vi.mock("./skillsEvolution", () => ({ evolveSkills: vi.fn() }));
vi.mock("./subagents", () => ({ tickSubagents: vi.fn() }));
vi.mock("./telegramBridge", () => ({ startTelegramBridge: vi.fn(), stopTelegramBridge: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

const MIN = 60_000;
const H = 60 * MIN;
// 08:00 Asia/Tokyo on 2026-10-30 is 23:00Z on 2026-10-29: the slot falls on the previous UTC date.
const slot = Date.UTC(2026, 9, 29, 23, 0, 0);
const DAY = 24 * H;

/** Every other job ran recently, so only the daily summary can be due. */
function seed(lastDaily: number) {
  m.store = JSON.stringify({
    dailySummaryHour: 8, lastDailySummaryTimestamp: lastDaily,
    lastHealthCheckTimestamp: slot + DAY, lastConsolidationTimestamp: slot + DAY, lastReasoningTimestamp: slot + DAY,
    lastPruneTimestamp: slot + DAY, lastMiddayTimestamp: slot + DAY, lastEveningTimestamp: slot + DAY,
  });
}

/** Launch the scheduler so its first tick (30 s after start) lands at `firstTick`. */
async function launch(firstTick: number) {
  vi.setSystemTime(firstTick - 30_000);
  vi.resetModules();
  const sched = await import("./scheduler");
  sched.startScheduler();
  await vi.advanceTimersByTimeAsync(30_000);
  return sched;
}

/** Advance (5-min ticks) until `at`. */
async function until(at: number) {
  await vi.advanceTimersByTimeAsync(at - Date.now());
}

const skipLogged = () => m.bus.complete.mock.calls.filter((c) => c[1] === "Skipped (missed window)").length;

describe("daily summary on time (Asia/Tokyo)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    m.daily.mockReset();
    m.bus.complete.mockClear();
    m.bus.suppress.mockClear();
    m.bus.fail.mockClear();
    m.daily.mockImplementation(async () => {
      if (m.sendsDisabled) return { sent: false, summary: "s", reason: SUPPRESSED };
      persist({ lastDailySummaryTimestamp: Date.now() });
      return { sent: true, summary: "s" };
    });
  });
  afterEach(() => vi.useRealTimers());

  it("runs under TZ=Asia/Tokyo with the slot on the previous UTC date", () => {
    expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe("Asia/Tokyo");
    expect(new Date(slot).getHours()).toBe(8);
    expect(new Date(slot).getUTCDate()).toBe(29);
  });

  for (const sendsDisabled of [false, true]) {
    it(`${sendsDisabled ? "sends disabled" : "sends enabled"}: once in the window, not after a relaunch, again the next day`, async () => {
      m.sendsDisabled = sendsDisabled;
      seed(slot - DAY + 2 * MIN); // ran yesterday on time

      // −1 min: not due. +4 min (next tick): runs once.
      let sched = await launch(slot - 1 * MIN);
      expect(m.daily).not.toHaveBeenCalled();
      sched.stopScheduler();

      sched = await launch(slot + 1 * MIN);
      expect(m.daily).toHaveBeenCalledTimes(1);
      if (sendsDisabled) {
        expect(m.bus.suppress).toHaveBeenCalledWith("t", "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)");
        expect(persisted().dailySummarySkippedSlot).toBe(slot);
      } else {
        expect(persisted().lastDailySummaryTimestamp).toBe(slot + 1 * MIN);
      }

      // +5 … +9 min: no re-run.
      await until(slot + 9 * MIN);
      expect(m.daily).toHaveBeenCalledTimes(1);

      // Relaunch at +10 min with the context reloaded from its persisted shape: no re-run.
      sched.stopScheduler();
      sched = await launch(slot + 10 * MIN);
      await until(slot + 29 * MIN);
      expect(m.daily).toHaveBeenCalledTimes(1);

      // Past the window: it ran (or was handled) for this slot, so nothing is logged as missed.
      await until(slot + 40 * MIN);
      expect(m.daily).toHaveBeenCalledTimes(1);
      expect(skipLogged()).toBe(0);
      expect(m.bus.fail).not.toHaveBeenCalled();

      // Next day's slot runs again (ticks fall at :x4.5/:x9.5 after the relaunch).
      await until(slot + DAY + 5 * MIN);
      expect(m.daily).toHaveBeenCalledTimes(2);
      sched.stopScheduler();
    });
  }

  it("+31 min on a day it never ran: skipped once and logged once, never generated", async () => {
    m.sendsDisabled = false;
    seed(slot - DAY + 2 * MIN);
    const sched = await launch(slot + 31 * MIN);
    await until(slot + 3 * H);
    sched.stopScheduler();
    expect(m.daily).not.toHaveBeenCalled();
    expect(skipLogged()).toBe(1);
    expect(persisted().dailySummarySkippedSlot).toBe(slot);
  });

  it("Tokyo midnight slot (hour 0) belongs to the new local day", async () => {
    const midnight = Date.UTC(2026, 9, 29, 15, 0, 0); // 00:00 JST 2026-10-30
    const { decideDueTasks } = await import("./scheduler");
    const ctx = { ...persisted(), dailySummaryHour: 0, lastDailySummaryTimestamp: midnight - 2 * MIN };
    const now = midnight + 1 * MIN;
    expect(decideDueTasks({ ...ctx, lastHealthCheckTimestamp: now }, midnight - 1 * MIN).map((t) => t.id)).not.toContain("daily");
    expect(decideDueTasks({ ...ctx, lastHealthCheckTimestamp: now }, now).map((t) => t.id)).toContain("daily");
    expect(decideDueTasks({ ...ctx, lastHealthCheckTimestamp: now, lastDailySummaryTimestamp: now }, now + 5 * MIN).map((t) => t.id)).not.toContain("daily");
  });
});
