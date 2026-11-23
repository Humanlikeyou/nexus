import { describe, expect, it } from "vitest";
import { BANDS, clockAt, hasSamples, sampleSessions, score, sessionStats } from "./sessionSamples";

describe("design-1110c Neuro history samples", () => {
  it("real sessions without samples have nothing to draw", () => {
    const real = { id: "r", device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:20:00+09:00", duration_secs: 1200, data_points: 76800, summary: { avg_focus: .62 } };
    expect(hasSamples(real)).toBe(false);
    expect(hasSamples({ ...real, samples: null })).toBe(false);
  });

  it("sample sessions are deterministic, flagged as samples and most recent first", () => {
    const a = sampleSessions(), b = sampleSessions();
    expect(a).toHaveLength(8);
    expect(a.every((s) => s.sample === true && s.id.startsWith("sample-") && hasSamples(s))).toBe(true);
    expect(a[0].start > a[1].start).toBe(true);
    expect(a[0].samples!.focus).toEqual(b[0].samples!.focus);
  });

  it("bands are shares of total power and scores stay inside 0–100", () => {
    const s = sampleSessions()[0].samples!;
    const total = BANDS.reduce((sum, { key }) => sum + s.bands[key][10], 0);
    expect(total).toBeCloseTo(100, 5);
    expect(Math.min(...s.focus, ...s.calm)).toBeGreaterThanOrEqual(0);
    expect(Math.max(...s.focus, ...s.calm)).toBeLessThanOrEqual(100);
    const stats = sessionStats(s);
    expect(BANDS.map((b) => b.key)).toContain(stats.strongest.key);
    expect(stats.contact).toBeGreaterThan(80);
  });

  it("scores read as 0–100 and time as m:ss", () => {
    expect(score(.62)).toBeCloseTo(62);
    expect(score(61)).toBe(61);
    expect(score(null)).toBeNull();
    expect(clockAt(25, 5)).toBe("2:05");
  });
});
