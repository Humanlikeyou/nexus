import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MetricReading } from "../../src-console/lib/mindBodyDeep";
import { BaselineChart, DeltaBar, SleepSpan, StageStrip, readingDelta, unitSuffix } from "../../src-console/screens/MindBodyCharts";

const reading = (key: string, unit: string, value: number | null, mean: number | null, z: number | null = null) => ({
  def: { key, label: key.toUpperCase(), unit, format: (v: number) => `${v}${unit}` },
  value, base: mean === null ? null : { mean, sd: 1 }, delta: null, z,
}) as unknown as MetricReading;

describe("unitSuffix", () => {
  it("formats the unit as it follows a number", () => {
    expect(unitSuffix(reading("hrv", " ms", 1, 1))).toBe(" ms");
    expect(unitSuffix(reading("recovery", "%", 1, 1))).toBe("%");
    expect(unitSuffix(reading("strain", "", 1, 1))).toBe("");
    expect(unitSuffix(reading("sleep", "min", 1, 1))).toBe("h");
  });
});

describe("readingDelta", () => {
  it("is null without a value or a baseline", () => {
    expect(readingDelta(reading("hrv", "ms", null, 50))).toBeNull();
    expect(readingDelta(reading("hrv", "ms", 50, null))).toBeNull();
  });
  it("describes today against his usual", () => {
    expect(readingDelta(reading("hrv", "ms", 56, 50))?.text).toBe("▲ 6 ms above usual");
    expect(readingDelta(reading("recovery", "%", 60, 70))?.text).toBe("▼ 10 pts below usual");
    expect(readingDelta(reading("sleep", "min", 450, 420))?.text).toBe("▲ 0.5h above usual");
    expect(readingDelta(reading("hrv", "ms", 50, 50))?.dir).toBe("flat");
  });
});

describe("DeltaBar", () => {
  it("draws nothing without a z score", () => {
    const { container } = render(<DeltaBar reading={reading("hrv", "ms", 1, 1, null)} />);
    expect(container.querySelector(".mb-delta__fill")).toBeNull();
  });
  it("fills to the right above usual, to the left below, and caps at 2.5 sd", () => {
    const up = render(<DeltaBar reading={reading("hrv", "ms", 1, 1, 1.25)} />);
    const fill = up.container.querySelector<HTMLElement>(".mb-delta__fill")!;
    expect(fill.dataset.side).toBe("right");
    expect(fill.style.width).toBe("25%");
    const down = render(<DeltaBar reading={reading("hrv", "ms", 1, 1, -9)} />);
    const low = down.container.querySelector<HTMLElement>(".mb-delta__fill")!;
    expect(low.dataset.side).toBe("left");
    expect(low.style.width).toBe("50%");
  });
});

describe("StageStrip", () => {
  it("renders nothing when no stage has minutes", () => {
    const { container } = render(<StageStrip stages={[{ label: "Deep", minutes: 0, tone: "deep" }]} />);
    expect(container.firstChild).toBeNull();
  });
  it("labels the strip with each stage's minutes and sizes segments by them", () => {
    const { container } = render(<StageStrip stages={[{ label: "Deep", minutes: 90.4, tone: "deep" }, { label: "REM", minutes: 60, tone: "rem" }]} />);
    expect(screen.getByRole("img", { name: "Deep 90 minutes, REM 60 minutes" })).toBeInTheDocument();
    const grows = Array.from(container.querySelectorAll<HTMLElement>("i")).map((i) => i.style.flexGrow);
    expect(grows).toEqual(["90.4", "60"]);
  });
});

describe("SleepSpan", () => {
  it("labels bed and wake in Tokyo time, across midnight", () => {
    render(<SleepSpan start="2026-11-09T14:00:00Z" end="2026-11-09T21:30:00Z" />); // 23:00 -> 06:30 JST
    expect(screen.getByRole("img", { name: "In bed 23:00 to 06:30" })).toBeInTheDocument();
    expect(screen.getByText("Bed 23:00")).toBeInTheDocument();
    expect(screen.getByText("Wake 06:30")).toBeInTheDocument();
  });
});

describe("BaselineChart", () => {
  it("says so when there are too few days to draw", () => {
    render(<BaselineChart points={[{ day: "2026-11-09", value: 50 }]} reading={reading("hrv", " ms", 50, 50)} />);
    expect(screen.getByText("Not enough days to draw yet")).toBeInTheDocument();
  });
  it("draws a labelled chart from real readings, skipping days without one", () => {
    const points = [{ day: "2026-11-07", value: 48 }, { day: "2026-11-08", value: null }, { day: "2026-11-09", value: 52 }];
    const { container } = render(<BaselineChart points={points as never} reading={reading("hrv", " ms", 52, 50)} />);
    expect(container.querySelector(".mb-chart")).not.toBeNull();
    expect(screen.getByRole("img", { name: /HRV, last 3 days, 2 readings, usual 50 ms/ })).toBeInTheDocument();
  });
});
