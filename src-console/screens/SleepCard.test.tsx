import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { NoopDailyMetric } from "../types";
import { SleepCard } from "./SleepCard";
import { sleepCardModel, takeaway } from "./sleepModel";

const day = (d: string, min: number | null, eff = .95): NoopDailyMetric => ({ day: d, source: "noop", totalSleepMin: min, efficiency: eff, deepMin: 60, remMin: 80, lightMin: 300, restingHr: 60, avgHrv: 30, recovery: 60, strain: 10, spo2Pct: null, skinTempDevC: null, respRateBpm: null });
// Shaped like Taisun's Nov 4–10 week: three long nights, then a normal one.
const days = [
  ...["10-20", "10-21", "10-22", "10-23", "10-24", "10-25", "10-26", "10-27", "10-28", "10-29", "10-30", "10-31", "11-01", "11-02", "11-03"].map((d) => day(`2026-${d}`, 480)),
  day("2026-11-04", 474), day("2026-11-05", 458), day("2026-11-06", 399), day("2026-11-07", 582), day("2026-11-08", 714), day("2026-11-09", 599), day("2026-11-10", 470, .92),
];
const night = { day: "2026-11-10", durationMin: 470, efficiency: .9186, startedAt: "2026-11-09T13:55:20Z", endedAt: "2026-11-09T22:27:09Z", deepMin: 64.5, remMin: 81.5, lightMin: 324.15, awakeMin: 42 };

describe("sleepCardModel", () => {
  const m = sleepCardModel(night, days);
  it("bed and wake are Tokyo clock times", () => { expect([m.bed, m.wake]).toEqual(["22:55", "07:27"]); });
  it("usual is the median of the prior nights only", () => { expect(m.usualMin).toBe(480); expect(m.usualEffPct).toBeCloseTo(95); });
  it("the week is 7 calendar nights ending tonight, gaps as null", () => {
    expect(m.week.map((w) => w.day)).toEqual(["2026-11-04", "2026-11-05", "2026-11-06", "2026-11-07", "2026-11-08", "2026-11-09", "2026-11-10"]);
    const gap = sleepCardModel(night, days.filter((d) => d.day !== "2026-11-06"));
    expect(gap.week[2].minutes).toBeNull();
  });
  it("the takeaway only states numbers shown on the card", () => {
    expect(m.takeaway).toBe("Normal length after 3 long nights (to 11h 54m) · 42 min awake.");
    expect(takeaway(400, 480, m.week, null)).toBe("1h 20m shorter than your usual.");
    expect(takeaway(470, null, m.week, 42)).toBeNull();
  });
});

describe("SleepCard", () => {
  it("shows duration, the vs-usual line, bed and wake, stages, efficiency and 7 nights", () => {
    render(<SleepCard model={sleepCardModel(night, days)} />);
    expect(screen.getByLabelText("7h 50m")).toBeInTheDocument();
    expect(screen.getByText("▼ 10 min vs usual 8h 00m")).toBeInTheDocument();
    expect(screen.getByText("22:55")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Deep 65 minutes, REM 82 minutes, Light 324 minutes, Awake 42 minutes" })).toBeInTheDocument();
    expect(screen.getByText(/Efficiency/).textContent).toMatch(/Efficiency 92% ▼ 3 vs 95%/);
    expect(screen.getByRole("img", { name: /^Last 7 nights: Nov 4 7h 54m/ })).toBeInTheDocument();
  });
  it("waits honestly with no record", () => {
    render(<SleepCard model={null} pending="Arrives after the sleep record closes" />);
    expect(screen.getByText("Arrives after the sleep record closes")).toBeInTheDocument();
  });
});
