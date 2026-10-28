/**
 * Ritual sends that fail (e.g. "Telegram not configured") back off 5/15/60 min,
 * max 3 tries per day, instead of retrying on every 5-minute scheduler tick
 * (the 2026-07-06 retry storm). TZ is pinned to Asia/Tokyo by `npm run test:unit`.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("../../src/terrance/system/activityBus", () => ({
  activityBus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));

let rituals: typeof import("../../src/terrance/system/rituals");
let backoff: typeof import("../../src/terrance/system/backoff");
const TICK = 5 * 60_000;

describe("ritual send backoff", () => {
  let sends: { id: string; at: number }[];
  let telegramOk: boolean;

  beforeEach(async () => {
    vi.resetModules();
    rituals = await import("../../src/terrance/system/rituals");
    backoff = await import("../../src/terrance/system/backoff");
    vi.useFakeTimers();
    sends = [];
    telegramOk = false;
    const byMessage = new Map(rituals.listRituals().map((r) => [r.message, r.id]));
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string, args?: { text?: string }) => {
      if (cmd === "load_kv") return null;
      if (cmd === "save_kv") return undefined;
      if (cmd === "send_telegram_message") {
        sends.push({ id: byMessage.get(args?.text ?? "") ?? "other", at: Date.now() });
        if (!telegramOk) throw new Error("Telegram not configured. Set bot token and chat ID in Settings.");
        return undefined;
      }
      throw new Error(`unexpected ${cmd}`);
    });
  });
  afterEach(() => vi.useRealTimers());

  /** Tick every 5 minutes from `start` for `ms`, like the scheduler does. */
  async function tickFor(start: string, ms: number) {
    vi.setSystemTime(new Date(start));
    for (let t = 0; t < ms; t += TICK) {
      await rituals.tickRituals();
      vi.advanceTimersByTime(TICK);
    }
  }

  it("a failing Social Expansion send is tried 3 times on the 5/15-min schedule, not every tick", async () => {
    // 15:00–17:55 JST: the ritual stays due the whole time (hour window 15–17).
    await tickFor("2026-10-28T15:00:00+09:00", 3 * 60 * 60_000);
    const times = sends.filter((s) => s.id === "social-expansion").map((s) => s.at);
    expect(times.length).toBe(backoff.MAX_ATTEMPTS_PER_WINDOW);
    for (let i = 1; i < times.length; i++) {
      expect(times[i] - times[i - 1]).toBeGreaterThanOrEqual(backoff.BACKOFF_DELAYS_MS[i - 1]);
      expect(times[i] - times[i - 1]).toBeLessThan(backoff.BACKOFF_DELAYS_MS[i - 1] + TICK);
    }
  });

  it("a send that succeeds after a failure stops further sends that day", async () => {
    vi.setSystemTime(new Date("2026-10-28T15:00:00+09:00"));
    await rituals.tickRituals();
    telegramOk = true;
    vi.advanceTimersByTime(TICK);
    await rituals.tickRituals();
    for (let i = 0; i < 20; i++) {
      vi.advanceTimersByTime(TICK);
      await rituals.tickRituals();
    }
    expect(sends.filter((s) => s.id === "social-expansion").length).toBe(2);
  });
});
