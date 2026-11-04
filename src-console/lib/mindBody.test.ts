import { describe, expect, it } from "vitest";
import type { NoopMetricPoint, RecordedSession } from "../types";
import { MIN_TREND_SESSIONS, baseline, matchDriver, mockSeriesPayload, mockSnapshot, pearson, readiness, trendText } from "./mindBody";
import { NoopShapeError, parseBodyContext, parseMetricSeries } from "./noop";

const series = (base: number[], today: number | null): NoopMetricPoint[] =>
  [...base, today].map((value, i) => ({ day: `2026-10-${String(i + 1).padStart(2, "0")}`, value }));
const flat = (v: number, n = 14) => Array.from({ length: n }, (_, i) => v + (i % 2 ? 2 : -2));

describe("readiness rules (own baseline)", () => {
  it("needs 7 baseline days", () => {
    expect(baseline([1, 2, 3, 4, 5, 6])).toBeNull();
    expect(baseline([1, 2, 3, 4, 5, 6, 7])?.n).toBe(7);
  });
  it("no data with fewer than 2 usable signals", () => {
    expect(readiness({ hrv: series(flat(60), 60), recovery: series([70], 70), sleep: [] }).level).toBe("none");
  });
  it("high when every signal is at or above baseline", () => {
    expect(readiness({ hrv: series(flat(60), 62), recovery: series(flat(70), 72), sleep: series(flat(450), 450) }).level).toBe("high");
  });
  it("low when HRV is ≥1 SD under baseline", () => {
    const r = readiness({ hrv: series(flat(60), 55), recovery: series(flat(70), 70), sleep: series(flat(450), 450) });
    expect(r.level).toBe("low");
    expect(r.hrvZ).toBeLessThanOrEqual(-1);
  });
  it("low when sleep is under 75% of usual or recovery 20 points down", () => {
    expect(readiness({ hrv: series(flat(60), 60), recovery: series(flat(70), 70), sleep: series(flat(450), 300) }).level).toBe("low");
    expect(readiness({ hrv: series(flat(60), 60), recovery: series(flat(70), 48), sleep: series(flat(450), 450) }).level).toBe("low");
  });
  it("moderate in between", () => {
    expect(readiness({ hrv: series(flat(60), 60), recovery: series(flat(70), 60), sleep: series(flat(450), 450) }).level).toBe("moderate");
  });
  it("is relative: the same HRV is high for one baseline and low for another", () => {
    const rest = { recovery: series(flat(70), 70), sleep: series(flat(450), 450) };
    expect(readiness({ hrv: series(flat(40), 50), ...rest }).level).toBe("high");
    expect(readiness({ hrv: series(flat(80), 50), ...rest }).level).toBe("low");
  });
});

describe("focus drivers (15-session threshold)", () => {
  const pts = (n: number) => Array.from({ length: n }, (_, i) => ({ x: i, y: i * 2 + (i % 3) }));
  it("shows the counter below 15", () => {
    expect(MIN_TREND_SESSIONS).toBe(15);
    expect(trendText(pts(0), "sleep")).toBe("Collecting data: 0/15 sessions");
    expect(trendText(pts(14), "sleep")).toBe("Collecting data: 14/15 sessions");
  });
  it("shows trend text at 15, labelled as correlation", () => {
    const t = trendText(pts(15), "sleep");
    expect(t).toMatch(/^Focus tends to be higher when sleep is higher/);
    expect(t).toContain("correlation, not causation");
  });
  it("pearson needs 3 points and variance", () => {
    expect(pearson(pts(2))).toBeNull();
    expect(pearson([{ x: 1, y: 1 }, { x: 1, y: 2 }, { x: 1, y: 3 }])).toBeNull();
  });
  it("prefers body_context, falls back to the day's NOOP value, skips unmatched", () => {
    const s = (start: string, ctx?: number): RecordedSession => ({ id: start, device_type: "muse", start, end: start, duration_secs: 60, data_points: 1, summary: { avg_focus: 50 },
      body_context: ctx === undefined ? undefined : { today: { recovery: null, hrv: ctx, rhr: null, strain: null, sleep_total_min: null }, latest_night: null } });
    const series = parseMetricSeries("hrv", mockSeriesPayload("hrv", [60, 61, 62])).points;
    const out = matchDriver([s("2026-10-08T09:00:00+09:00", 99), s("2026-10-09T09:00:00+09:00"), s("2020-01-01T09:00:00+09:00")], "hrv", series);
    expect(out).toHaveLength(2);
    expect(out[0].x).toBe(99);
  });
});

describe("NOOP payload parsers (real shapes)", () => {
  it("metric_series: empty points is an empty series, not an error", () => {
    expect(parseMetricSeries("hrv", mockSeriesPayload("hrv", [])).points).toEqual([]);
  });
  it("metric_series: wrong shape throws a shape error", () => {
    expect(() => parseMetricSeries("hrv", [{ day: "x", value: 1 }])).toThrow(NoopShapeError);
    expect(() => parseMetricSeries("hrv", null)).toThrow(NoopShapeError);
  });
  it("body_context (minimised): today metrics, sleep from latest_night", () => {
    const ctx = parseBodyContext({ today: { recovery: 71, hrv: 64, rhr: 51, strain: 9, sleep_total_min: 100 }, latest_night: { day: "2026-10-28", total_sleep_min: 455, efficiency: 91, deep_min: 90, rem_min: 100, light_min: 230 } });
    expect(ctx).toEqual({ captured_at: "2026-10-28", recovery: 71, hrv_ms: 64, resting_hr: 51, sleep_minutes: 455 });
  });
  it("body_context: sleep falls back to today; null when absent, wrong shape or all-null", () => {
    expect(parseBodyContext({ today: { recovery: null, hrv: null, rhr: null, strain: null, sleep_total_min: 420 }, latest_night: null })?.sleep_minutes).toBe(420);
    expect(parseBodyContext(undefined)).toBeNull();
    expect(parseBodyContext(mockSnapshot(2))).toBeNull();
    expect(parseBodyContext({ today: null, latest_night: null })).toBeNull();
  });
});
