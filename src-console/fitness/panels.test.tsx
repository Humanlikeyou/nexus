import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { INITIAL_SNAPSHOT } from "../bridge";
import { SAMPLE_ADAPTERS_KEY } from "../lib/sampleAdapters";
import { HealthField } from "../screens/HealthScreen";
import type { NoopDailyMetric, NoopHealthSnapshot, SystemSnapshot } from "../types";
import { SleepDetailPanel } from "./SleepDetailPanel";
import { TrendsExplorer } from "./TrendsExplorer";
import { WorkoutsPanel } from "./WorkoutsPanel";

const sampleOn = () => localStorage.setItem(SAMPLE_ADAPTERS_KEY, "1");
beforeEach(() => { invoke.mockReset(); localStorage.removeItem(SAMPLE_ADAPTERS_KEY); vi.spyOn(console, "warn").mockImplementation(() => {}); });

const workout = (over = {}) => ({ day: "2026-11-10", startedAt: "2026-11-10T07:10:00+09:00", endedAt: "2026-11-10T07:55:00+09:00", sport: "running", durationMin: 45, energyKcal: 300, avgHr: 140, maxHr: 170, strain: 9, distanceM: 6000, steps: 7000, zoneMinutes: [5, 15, 15, 8, 2], ...over });

describe("WorkoutsPanel", () => {
  it("loading, then live rows with zone minutes as text", async () => {
    invoke.mockResolvedValue([workout(), workout({ startedAt: "2026-11-09T07:00:00+09:00", day: "2026-11-09", sport: "aikido", zoneMinutes: null, distanceM: null })]);
    render(<WorkoutsPanel />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading workouts");
    expect(await screen.findByRole("list", { name: "Workouts" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Running" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Running heart-rate zones: Zone 1 5m, Zone 2 15m, Zone 3 15m, Zone 4 8m, Zone 5 2m" })).toBeInTheDocument();
    expect(screen.getByText("No zone data for this session.")).toBeInTheDocument();
    expect(screen.getByText(/1 session has no zone data/)).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
  });
  it("changing the range asks again with that many days", async () => {
    invoke.mockResolvedValue([workout()]);
    render(<WorkoutsPanel />);
    await screen.findByRole("list", { name: "Workouts" });
    fireEvent.click(screen.getByRole("radio", { name: "3 months" }));
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("noop_recent_workouts", { days: 90 }, undefined));
  });
  it("empty: honest sentence, no numbers", async () => {
    invoke.mockResolvedValue([]);
    const { container } = render(<WorkoutsPanel />);
    expect(await screen.findByText("No workouts yet.")).toBeInTheDocument();
    expect(container.querySelector(".ui-meter")).toBeNull();
  });
  it("unwired says Needs wiring and offers Check again", async () => {
    invoke.mockRejectedValue("NOOP has not created its local health store yet.");
    render(<WorkoutsPanel />);
    expect(await screen.findByText("Needs wiring")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check again" })).toBeInTheDocument();
  });
  it("error: plain sentence, never the backend's text; Retry reads again", async () => {
    invoke.mockRejectedValueOnce("NOOP store could not be opened: /Users/x/secret.sqlite").mockResolvedValue([workout()]);
    const { container } = render(<WorkoutsPanel />);
    expect(await screen.findByRole("alert")).toHaveTextContent("couldn't read workouts");
    expect(container.textContent).not.toMatch(/secret|sqlite|NOOP store/);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("list", { name: "Workouts" })).toBeInTheDocument();
  });
  it("sample: tagged Sample data, no backend call", async () => {
    sampleOn();
    render(<WorkoutsPanel />);
    expect(await screen.findByRole("list", { name: "Workouts" })).toBeInTheDocument();
    expect(screen.getByText("Sample data")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalled();
  });
});

const night = (day: string, min: number): NoopDailyMetric => ({ day, source: "n", totalSleepMin: min, efficiency: 0.9, deepMin: 80, remMin: 90, lightMin: 250, restingHr: 52, avgHrv: 60, recovery: 70, strain: 10, spo2Pct: null, skinTempDevC: null, respRateBpm: null });
const days = ["2026-11-06", "2026-11-07", "2026-11-08", "2026-11-09", "2026-11-10"].map((d, i) => night(d, 420 + i * 10));
const T0 = Date.parse("2026-11-09T23:00:00+09:00") / 1000;
const hypno = { day: "2026-11-10", startedAt: "", endedAt: "", segments: [{ start: T0, end: T0 + 3600, stage: "light" }, { start: T0 + 3600, end: T0 + 7200, stage: "deep" }, { start: T0 + 7200, end: T0 + 7800, stage: "awake" }, { start: T0 + 7800, end: T0 + 9600, stage: "rem" }] };

describe("SleepDetailPanel", () => {
  it("live: hypnogram with a text alternative, stage breakdown, 14-night charts", async () => {
    invoke.mockResolvedValue(hypno);
    render(<SleepDetailPanel days={days} />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading sleep stages");
    const chart = await screen.findByRole("img", { name: /^Sleep stages from 23:00 to 01:40: Deep 1h 0m, Light 1h 0m, REM 30m, Awake 10m; 1 awakening\.$/ });
    expect(chart).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Time in each sleep stage: Deep 1h 0m/ })).toBeInTheDocument();
    expect(screen.getByText("Stage timeline as text")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /^Time asleep, last 14 nights/ })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("noop_hypnogram", { day: null }, undefined);
  });
  it("picking a night asks for that day", async () => {
    invoke.mockResolvedValue(hypno);
    render(<SleepDetailPanel days={days} />);
    await screen.findByRole("img", { name: /^Sleep stages/ });
    fireEvent.change(screen.getByRole("combobox", { name: "Night to show" }), { target: { value: "2026-11-08" } });
    await waitFor(() => expect(invoke).toHaveBeenLastCalledWith("noop_hypnogram", { day: "2026-11-08" }, undefined));
  });
  it("empty: no segments means no chart and an honest line; 14-night card still works from the snapshot", async () => {
    invoke.mockResolvedValue(null);
    render(<SleepDetailPanel days={days} />);
    expect(await screen.findByText("No sleep stages yet.")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /^Sleep stages/ })).toBeNull();
    expect(screen.getByRole("img", { name: /^Time asleep/ })).toBeInTheDocument();
  });
  it("unwired and error states", async () => {
    invoke.mockRejectedValueOnce("NOOP has not created its local health store yet.");
    const { unmount } = render(<SleepDetailPanel days={[]} />);
    expect(await screen.findByText("Needs wiring")).toBeInTheDocument();
    expect(screen.getByText("Nightly charts appear after two nights of sleep records.")).toBeInTheDocument();
    unmount();
    invoke.mockRejectedValueOnce("boom");
    render(<SleepDetailPanel days={days} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("couldn't read sleep stages");
  });
  it("sample: tagged", async () => {
    sampleOn();
    render(<SleepDetailPanel days={days} />);
    expect(await screen.findByRole("img", { name: /^Sleep stages/ })).toBeInTheDocument();
    expect(screen.getByText("Sample data")).toBeInTheDocument();
  });
});

const seriesFor = (key: string) => ({ key, points: Array.from({ length: 40 }, (_, i) => ({ day: new Date(Date.UTC(2026, 9, 1 + i)).toISOString().slice(0, 10), value: key === "sleep_total_min" ? 400 + i : 50 + (i % 9) })) });
const answer = () => invoke.mockImplementation(async (_c: string, a: { args: { key: string } }) => seriesFor(a.args.key));

describe("TrendsExplorer", () => {
  it("live: a chart with a text alternative, a metric picker with all 9, a tile per metric", async () => {
    answer();
    render(<TrendsExplorer />);
    const chart = await screen.findByRole("img", { name: /^Recovery, last 30 days: lowest/ });
    expect(chart).toBeInTheDocument();
    expect(within(screen.getByLabelText("Metric")).getAllByRole("option")).toHaveLength(9);
    expect(within(screen.getByRole("list", { name: "All metrics" })).getAllByRole("button")).toHaveLength(9);
    expect(screen.queryByText("Sample data")).toBeNull();
  });
  it("choosing a metric (picker or tile) and a range changes the chart", async () => {
    answer();
    render(<TrendsExplorer />);
    await screen.findByRole("img", { name: /^Recovery, last 30 days/ });
    fireEvent.click(screen.getByRole("button", { name: /^Sleep/ }));
    expect(await screen.findByRole("img", { name: /^Sleep, last 30 days: lowest 6\.8 h/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Sleep/ })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("radio", { name: "Week" }));
    expect(await screen.findByRole("img", { name: /^Sleep, last 7 days/ })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Metric"), { target: { value: "steps" } });
    expect(await screen.findByRole("img", { name: /^Steps, last 7 days/ })).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledTimes(9); // the range only slices what was read
  });
  it("a metric with no readings says so, the others still draw", async () => {
    invoke.mockImplementation(async (_c: string, a: { args: { key: string } }) => (a.args.key === "spo2" ? { key: "spo2", points: [] } : seriesFor(a.args.key)));
    render(<TrendsExplorer />);
    await screen.findByRole("img", { name: /^Recovery, last/ });
    fireEvent.change(screen.getByLabelText("Metric"), { target: { value: "spo2" } });
    expect(await screen.findByText("No blood oxygen readings yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Blood oxygen\s*No readings/ })).toBeInTheDocument();
  });
  it("empty, unwired, error", async () => {
    invoke.mockImplementation(async (_c: string, a: { args: { key: string } }) => ({ key: a.args.key, points: [] }));
    const a = render(<TrendsExplorer />);
    expect(await screen.findByText("No trends yet.")).toBeInTheDocument();
    a.unmount();
    invoke.mockReset(); invoke.mockRejectedValue({ kind: "binary_missing", detail: "x" });
    const b = render(<TrendsExplorer />);
    expect(await screen.findByText("Needs wiring")).toBeInTheDocument();
    b.unmount();
    invoke.mockReset(); invoke.mockRejectedValue({ kind: "timeout", detail: "x" });
    render(<TrendsExplorer />);
    expect(await screen.findByRole("alert")).toHaveTextContent("couldn't read trends");
  });
  it("sample: tagged, no calls", async () => {
    sampleOn();
    render(<TrendsExplorer />);
    expect(await screen.findByRole("img", { name: /^Recovery, last/ })).toBeInTheDocument();
    expect(screen.getByText("Sample data")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("Fitness tabs", () => {
  const dm = (d: string): NoopDailyMetric => night(d, 450);
  const ready: Partial<NoopHealthSnapshot> = { status: "ready", generatedAt: "2026-11-10T07:30:00+09:00", dataAgeDays: 0, deviceId: "noop-1", latestDay: "2026-11-10", today: dm("2026-11-10"), recentDays: [dm("2026-11-08"), dm("2026-11-09")], latestNight: null, latestSleep: null, battery: null, insights: [], message: null };
  const data: SystemSnapshot = { ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, noop: { ...INITIAL_SNAPSHOT.noop, ...ready } };
  it("Summary is the default and the other three tabs open their panels", async () => {
    invoke.mockResolvedValue([]);
    const { container } = render(<HealthField data={data} />);
    const tabs = screen.getByRole("radiogroup", { name: "Fitness section" });
    expect(within(tabs).getAllByRole("radio").map((r) => r.getAttribute("aria-label") ?? r.textContent)).toEqual(["Summary", "Workouts", "Sleep detail", "Trends"]);
    expect(within(tabs).getByRole("radio", { name: "Summary" })).toBeChecked();
    expect(container.querySelector(".fitness__row--today")).not.toBeNull();
    fireEvent.click(within(tabs).getByRole("radio", { name: "Workouts" }));
    expect(container.querySelector(".fitness__row--today")).toBeNull();
    expect(await screen.findByRole("heading", { name: "Workouts" })).toBeInTheDocument();
    fireEvent.click(within(tabs).getByRole("radio", { name: "Sleep detail" }));
    expect(await screen.findByRole("heading", { name: "Sleep stages" })).toBeInTheDocument();
    fireEvent.click(within(tabs).getByRole("radio", { name: "Trends" }));
    expect(await screen.findByRole("heading", { name: "Trends" })).toBeInTheDocument();
    fireEvent.click(within(tabs).getByRole("radio", { name: "Summary" }));
    expect(container.querySelector(".fitness__row--today")).not.toBeNull();
  });
  it("no tab bar when the health store has nothing (empty state is unchanged)", () => {
    render(<HealthField data={{ ...data, noop: { ...data.noop, status: "waiting_for_data" } }} />);
    expect(screen.queryByRole("radiogroup", { name: "Fitness section" })).toBeNull();
  });
});
