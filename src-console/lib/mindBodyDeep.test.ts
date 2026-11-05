import { describe, expect, it } from "vitest";
import { METRICS, baselineFor, chartGeometry, driverSentence, drivers, hoursMinutes, inRange, lean, loadRatio, mergeSeries, metricReading, windowAverage } from "./mindBodyDeep";

const series = (values: Array<number | null>) => values.map((value, i) => ({ day: `2026-10-${String(i + 1).padStart(2, "0")}`, value }));
const steady = (n: number, value: number, wobble = 2) => series(Array.from({ length: n }, (_, i) => value + (i % 2 ? wobble : -wobble)));

describe("baseline", () => {
  it("uses the days before the reading, never the reading itself", () => {
    const points = [...steady(20, 60), { day: "2026-10-21", value: 200 }];
    expect(baselineFor(points, "2026-10-21")?.mean).toBeCloseTo(60, 0);
  });
  it("needs a week of history", () => {
    expect(baselineFor(steady(5, 60), null)).toBeNull();
  });
  it("reads a metric against its usual", () => {
    const reading = metricReading(METRICS.hrv, [...steady(20, 60), { day: "2026-10-21", value: 70 }], "2026-10-21");
    expect(reading.delta).toBeCloseTo(10, 0);
    expect(lean(reading)).toBe("up");
  });
  it("a higher resting heart rate leans down, and strain leans nowhere", () => {
    const rhr = metricReading(METRICS.resting_hr, [...steady(20, 50, 1), { day: "2026-10-21", value: 60 }], "2026-10-21");
    const strain = metricReading(METRICS.strain, [...steady(20, 10, 1), { day: "2026-10-21", value: 18 }], "2026-10-21");
    expect(lean(rhr)).toBe("down");
    expect(lean(strain)).toBe("flat");
  });
  it("a missing day is no reading, not a zero", () => {
    const reading = metricReading(METRICS.hrv, [...steady(20, 60), { day: "2026-10-21", value: null }], "2026-10-21");
    expect(reading.value).toBeNull();
    expect(reading.delta).toBeNull();
  });
});

describe("what's driving today", () => {
  const today = (values: Partial<Record<"sleep" | "hrv", number>>) => [
    metricReading(METRICS.sleep, [...steady(20, 440, 10), { day: "2026-10-21", value: values.sleep ?? 440 }], "2026-10-21"),
    metricReading(METRICS.hrv, [...steady(20, 60, 3), { day: "2026-10-21", value: values.hrv ?? 60 }], "2026-10-21"),
  ];
  it("names the biggest mover first, in his own units", () => {
    const readings = today({ sleep: 320 });
    expect(drivers(readings)[0].def.key).toBe("sleep");
    expect(driverSentence(readings)).toBe("Mostly sleep, 120 min below your usual.");
  });
  it("says so when nothing moved", () => {
    expect(driverSentence(today({}))).toBe("Everything sits close to your usual today.");
  });
  it("says nothing without a baseline", () => {
    expect(driverSentence([metricReading(METRICS.hrv, steady(3, 60), null)])).toBeNull();
  });
  it("never uses medical language", () => {
    expect(driverSentence(today({ sleep: 200, hrv: 30 }))).not.toMatch(/diagnos|disease|treat|symptom|illness/i);
  });
});

describe("windows and charts", () => {
  it("averages a window ending at the reading day, skipping gaps", () => {
    expect(windowAverage(series([10, null, 20, 99]), "2026-10-03", 7)).toBe(15);
  });
  it("compares the week with the month", () => {
    expect(loadRatio([...steady(21, 10, 0), ...series([]), ...Array.from({ length: 7 }, (_, i) => ({ day: `2026-10-${22 + i}`, value: 20 }))], null)?.line).toMatch(/above your usual/);
    expect(loadRatio(series([1, 2]), null)?.ratio).toBeDefined();
    expect(loadRatio([], null)).toBeNull();
  });
  it("a range ends at the reading day", () => {
    expect(inRange(series([1, 2, 3, 4]), "2026-10-03", 2).map((p) => p.value)).toEqual([2, 3]);
  });
  it("merging prefers the snapshot, and keeps long history", () => {
    expect(mergeSeries(series([1, 2]), [{ day: "2026-10-02", value: 9 }]).map((p) => p.value)).toEqual([1, 9]);
    expect(mergeSeries(series([1, 2]), [{ day: "2026-10-02", value: null }]).map((p) => p.value)).toEqual([1, 2]);
  });
  it("a gap breaks the line instead of bridging it", () => {
    expect(chartGeometry(series([1, 2, null, 4, 5]), null).path.match(/M/g)).toHaveLength(2);
    expect(chartGeometry([], null).path).toBe("");
  });
  it("formats sleep as hours and minutes", () => {
    expect(hoursMinutes(452)).toBe("7h 32m");
    expect(hoursMinutes(60)).toBe("1h 00m");
  });
});
