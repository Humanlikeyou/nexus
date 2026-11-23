import { describe, expect, it } from "vitest";
import type { Hypnogram, NoopWorkout } from "../bridge";
import type { NoopDailyMetric } from "../types";
import { hypnogramModel, nightOptions, nightSeries } from "./sleepDetailModel";
import { METRICS, METRIC_BY_KEY, endDay, metricStats, windowSeries, withUnit } from "./trendsModel";
import { sportLabel, workoutRows, workoutSummary, zoneText } from "./workoutsModel";

const wo = (over: Partial<NoopWorkout> = {}): NoopWorkout => ({
  day: "2026-11-10", startedAt: "2026-11-10T07:10:00+09:00", endedAt: "2026-11-10T07:55:00+09:00", sport: "functional_strength",
  durationMin: 45, energyKcal: 310.4, avgHr: 131.2, maxHr: 168, strain: 9.4, distanceM: null, steps: null, zoneMinutes: [5, 15, 15, 8, 2], ...over,
});

describe("workouts mapping", () => {
  it("sport ids become sentence case", () => {
    expect(sportLabel("functional_strength")).toBe("Functional strength");
    expect(sportLabel("hiit")).toBe("HIIT");
    expect(sportLabel("")).toBe("Workout");
    expect(sportLabel(null)).toBe("Workout");
  });
  it("rows are newest first, with a JST time, a day label and one facts line", () => {
    const rows = workoutRows([wo({ startedAt: "2026-11-08T07:00:00+09:00", day: "2026-11-08" }), wo({ distanceM: 5200, sport: "running" })]);
    expect(rows.map((r) => r.sport)).toEqual(["Running", "Functional strength"]);
    expect(rows[0]).toMatchObject({ time: "07:10", dayLabel: "Tue, Nov 10", duration: "45m", distanceKm: 5.2 });
    expect(rows[0].facts).toBe("45m · 5.2 km · 310 kcal · avg 131 bpm · max 168 bpm");
  });
  it("zones: null, empty and all-zero are 'no zone data', never zeros; short arrays pad", () => {
    expect(workoutRows([wo({ zoneMinutes: null })])[0].zones).toBeNull();
    expect(workoutRows([wo({ zoneMinutes: [] })])[0].zones).toBeNull();
    expect(workoutRows([wo({ zoneMinutes: [0, 0, 0, 0, 0] })])[0].zones).toBeNull();
    expect(workoutRows([wo({ zoneMinutes: [3, 4] })])[0].zones).toEqual([3, 4, 0, 0, 0]);
  });
  it("missing numbers stay missing", () => {
    const r = workoutRows([wo({ durationMin: null, energyKcal: null, avgHr: null, maxHr: null })])[0];
    expect([r.minutes, r.duration, r.kcal, r.avgHr, r.maxHr]).toEqual([null, null, null, null, null]);
    expect(r.facts).toBe("");
  });
  it("summary totals only sessions that have zones", () => {
    const rows = workoutRows([wo(), wo({ zoneMinutes: null, startedAt: "2026-11-09T07:00:00+09:00" })]);
    const s = workoutSummary(rows);
    expect(s).toMatchObject({ sessions: 2, minutes: 90, zoneTotals: [5, 15, 15, 8, 2], zoneMinutes: 45, withZones: 1 });
    expect(zoneText([5, 15, 15, 8, 2])).toBe("Zone 1 5m, Zone 2 15m, Zone 3 15m, Zone 4 8m, Zone 5 2m");
  });
});

const T0 = Date.parse("2026-11-09T23:00:00+09:00") / 1000;
const hyp = (segments: Array<[string, number]>): Hypnogram => {
  let t = T0;
  return { day: "2026-11-10", startedAt: "", endedAt: "", segments: segments.map(([stage, min]) => { const s = { start: t, end: t + min * 60, stage: stage as never }; t += min * 60; return s; }) };
};

describe("hypnogram mapping", () => {
  it("totals minutes per stage, counts awakenings inside the night and writes a summary", () => {
    const m = hypnogramModel(hyp([["light", 30], ["deep", 60], ["awake", 10], ["rem", 40], ["light", 20], ["awake", 5]]))!;
    expect(m.minutes).toEqual({ awake: 15, rem: 40, light: 50, deep: 60 });
    expect(m.awakenings).toBe(1); // the trailing awake stretch is waking up, not an awakening
    expect([m.bed, m.wake]).toEqual(["23:00", "01:45"]);
    expect(m.blocks[1]).toMatchObject({ stage: "deep", row: 3, minutes: 60 });
    expect(Math.round(m.pct.deep)).toBe(36);
    expect(m.summary).toBe("Sleep stages from 23:00 to 01:45: Deep 1h 0m, Light 50m, REM 40m, Awake 15m; 1 awakening.");
  });
  it("no usable segments is null (an empty state, not a flat chart)", () => {
    expect(hypnogramModel({ day: "d", startedAt: "", endedAt: "", segments: [] })).toBeNull();
    expect(hypnogramModel({ day: "d", startedAt: "", endedAt: "", segments: [{ start: 5, end: 5, stage: "deep" }] })).toBeNull();
  });
});

const night = (day: string, min: number | null, eff: number | null = 0.9): NoopDailyMetric => ({ day, source: "n", totalSleepMin: min, efficiency: eff, deepMin: null, remMin: null, lightMin: null, restingHr: null, avgHrv: null, recovery: null, strain: null, spo2Pct: null, skinTempDevC: null, respRateBpm: null });

describe("night lists", () => {
  const days = [night("2026-11-07", 400), night("2026-11-08", 0), night("2026-11-09", 480, null), night("2026-11-10", 450)];
  it("options are newest first and skip nights with no reading (0 is missing)", () => {
    expect(nightOptions(days).map((n) => n.day)).toEqual(["2026-11-10", "2026-11-09", "2026-11-07"]);
    expect(nightOptions(days)[0].label).toBe("Tue, Nov 10 · 7h 30m");
  });
  it("series keeps calendar gaps as null, efficiency as percent", () => {
    const s = nightSeries(days, 4);
    expect(s.hours).toEqual([400 / 60, null, 8, 7.5]);
    expect(s.efficiencyPct).toEqual([90, null, null, 90]);
    expect(s.nights).toBe(3);
  });
});

describe("trends mapping", () => {
  it("covers exactly the 9 metric_series keys", () => {
    expect(METRICS.map((m) => m.key).sort()).toEqual(["active_kcal", "hrv", "recovery", "resp_rate", "rhr", "sleep_total_min", "spo2", "steps", "strain"]);
  });
  const pts = [{ day: "2026-11-08", value: 400 }, { day: "2026-11-09", value: 0 }, { day: "2026-11-10", value: 480 }];
  it("sleep minutes become hours, zero is a gap, missing days are null", () => {
    const s = windowSeries(METRIC_BY_KEY.sleep_total_min, pts, 4, "2026-11-10");
    expect(s.map((x) => x.value)).toEqual([null, 400 / 60, null, 8]);
  });
  it("steps keep a real zero", () => {
    expect(windowSeries(METRIC_BY_KEY.steps, [{ day: "2026-11-10", value: 0 }], 1, "2026-11-10")[0].value).toBe(0);
  });
  it("stats ignore gaps; all-gap windows have no latest", () => {
    expect(metricStats([{ day: "a", value: 2 }, { day: "b", value: null }, { day: "c", value: 6 }])).toEqual({ latest: 6, mean: 4, min: 2, max: 6, count: 2 });
    expect(metricStats([{ day: "a", value: null }]).latest).toBeNull();
  });
  it("endDay is the newest day with a value; formatting adds units", () => {
    expect(endDay([[{ day: "2026-11-01", value: 1 }], [{ day: "2026-11-05", value: null }, { day: "2026-11-03", value: 2 }]])).toBe("2026-11-03");
    expect(endDay([[]])).toBeNull();
    expect(withUnit(METRIC_BY_KEY.recovery, 71.4)).toBe("71%");
    expect(withUnit(METRIC_BY_KEY.steps, 8200)).toBe("8,200");
    expect(withUnit(METRIC_BY_KEY.hrv, null)).toBe("No reading");
  });
});
