import { beforeEach, describe, expect, it, vi } from "vitest";

const noop = vi.hoisted(() => ({ query: vi.fn() }));
vi.mock("./noop", async (original) => ({ ...(await original<typeof import("./noop")>()), noopQuery: noop.query }));

import { loadReadinessLine } from "./useReadiness";

const row = (day: string, recovery: number | null, hrv: number | null, sleep: number | null) => ({ day, recovery, avgHrv: hrv, totalSleepMin: sleep, source: "", efficiency: null, deepMin: null, remMin: null, lightMin: null, disturbances: null, restingHr: null, strain: null, exerciseCount: null, spo2Pct: null, skinTempDevC: null, respRateBpm: null, steps: null, activeKcalEst: null });
const days = Array.from({ length: 20 }, (_, i) => row(`2026-10-${String(i + 1).padStart(2, "0")}`, 70, 60, 440));
const snapshot = (today: ReturnType<typeof row> | null) => ({ generatedAt: "x", logicalToday: "x", sources: {}, freshness: {}, recentDays: days, appleDaily: [], today });

describe("Today's readiness line", () => {
  beforeEach(() => noop.query.mockReset());
  it("gives the score and its one-line reason", async () => {
    noop.query.mockResolvedValue({ ok: true, value: snapshot(row("2026-10-21", 30, 60, 440)) });
    expect(await loadReadinessLine()).toMatchObject({ score: 30, label: "Low", reason: "Recovery is low. Favour rest or light work.", earlierDay: null });
  });
  it("names the day when today hasn't synced yet", async () => {
    noop.query.mockResolvedValue({ ok: true, value: snapshot(row("2026-10-21", null, null, null)) });
    expect((await loadReadinessLine())?.earlierDay).toBe("Oct 20");
  });
  it("shows nothing when NOOP can't be read or there's no baseline", async () => {
    noop.query.mockResolvedValue({ ok: false, error: { kind: "timeout", detail: "" } });
    expect(await loadReadinessLine()).toBeNull();
    noop.query.mockResolvedValue({ ok: true, value: { ...snapshot(row("2026-10-21", 70, 60, 440)), recentDays: [] } });
    expect(await loadReadinessLine()).toBeNull();
  });
});
