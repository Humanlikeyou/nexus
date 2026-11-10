import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { NoopDailyMetric, NoopHealthSnapshot, SystemSnapshot } from "../types";
import { HealthField, buildInsights, recoveryBand, signed } from "./HealthScreen";

const day = (d: string, over: Partial<NoopDailyMetric> = {}): NoopDailyMetric => ({
  day: d, source: "noop", totalSleepMin: 452, efficiency: 0.91, deepMin: 92, remMin: 104, lightMin: 230,
  restingHr: 52, avgHrv: 64, recovery: 74, strain: 11.3, spo2Pct: null, skinTempDevC: null, respRateBpm: null, ...over,
});

function snap(noop: Partial<NoopHealthSnapshot>, extra: Partial<SystemSnapshot> = {}): SystemSnapshot {
  return { ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, noop: { ...INITIAL_SNAPSHOT.noop, ...noop }, ...extra };
}

const ready: Partial<NoopHealthSnapshot> = {
  status: "ready", generatedAt: "2026-10-28T07:30:00+09:00", dataAgeDays: 0, deviceId: "noop-1", latestDay: "2026-10-28",
  today: day("2026-10-28", { strain: 125 }),
  recentDays: [day("2026-10-26", { recovery: 60 }), day("2026-10-27", { recovery: null }), day("2026-10-28")],
  latestNight: { day: "2026-10-28", startedAt: "2026-10-27T23:41:00+09:00", endedAt: "2026-10-28T07:13:00+09:00", durationMin: 452, efficiency: 0.91, deepMin: 92, remMin: 104, lightMin: 230, awakeMin: 26, sessionCount: 2, restingHr: 51, avgHrv: 66 },
  latestSleep: null,
  battery: { percentage: 82, charging: false, observedAt: "2026-10-28T07:00:00+09:00" },
  insights: [],
  message: null,
};

describe("Fitness (HealthField)", () => {
  it("loading: skeleton with heading, no dashes", () => {
    const { container } = render(<HealthField data={INITIAL_SNAPSHOT} />);
    expect(screen.getByRole("heading", { level: 1, name: "Fitness" })).toBeInTheDocument();
    expect(container.querySelector("[aria-busy=true]")).not.toBeNull();
    expect(container.textContent).not.toContain("—");
    expect(container.textContent).not.toContain("--");
  });

  it("empty (waiting_for_data): first-record card, NOOP copy, no WHOOP", () => {
    const { container } = render(<HealthField data={snap({ status: "waiting_for_data", message: "No local store yet." })} />);
    expect(screen.getByRole("heading", { name: "Waiting for the first NOOP record" })).toBeInTheDocument();
    expect(screen.getByText("No local store yet.")).toBeInTheDocument();
    expect(screen.getByText("Waiting for data")).toBeInTheDocument();
    expect(container.textContent).not.toMatch(/whoop/i);
  });

  it("empty: Check again action, null tiles hide units, readiness values are badges", () => {
    const onRetry = vi.fn();
    const { container } = render(<HealthField onRetry={onRetry} data={snap({ status: "waiting_for_data" })} />);
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(onRetry).toHaveBeenCalledOnce();
    expect(container.querySelectorAll(".ui-stat__value[data-empty]")).toHaveLength(3);
    expect(screen.queryByText("ms")).toBeNull();
    expect(screen.getByText("Store missing")).toHaveClass("ui-badge");
    expect(screen.getByText("Amara has no NOOP daily record to read yet.")).toBeInTheDocument();
  });

  it("error: NOOP command unavailable shows Unavailable badge and Retry", () => {
    const onRetry = vi.fn();
    render(<HealthField onRetry={onRetry} data={snap(ready, { unavailableSources: [{ command: "get_noop_health_snapshot", label: "NOOP health" }] })} />);
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
    expect(screen.queryByRole("meter")).toBeNull();
  });

  it("ready: rings, band word, clamped effort, NOOP band battery, merged sessions, insights", () => {
    const { container } = render(<HealthField data={snap(ready)} />);
    expect(screen.getByText(/^Updated (today|yesterday|\d+ days ago) at /)).toBeInTheDocument();
    const recovery = screen.getByRole("meter", { name: "Recovery" });
    expect(recovery).toHaveAttribute("aria-valuenow", "74");
    expect(recovery).toHaveAttribute("aria-valuetext", "74%, High");
    expect(screen.getByText("High")).toBeInTheDocument();
    const effort = screen.getByRole("meter", { name: "Effort" });
    expect(effort).toHaveAttribute("aria-valuenow", "100");
    expect(effort).toHaveAttribute("aria-valuetext", "100 of 100");
    expect(screen.getByRole("radio", { name: "Effort" })).toBeInTheDocument();
    expect(screen.getByText(/NOOP band 82% · On strap/)).toHaveClass("fitness__status");
    // No duplicate numbers: Row 2 (SpO2/skin/resp) is hidden when all are null, Last night keeps RHR + HRV only.
    expect(screen.queryByText("Duration")).toBeNull();
    expect(screen.getAllByText("Resting HR", { selector: "dt" })).toHaveLength(1);
    expect(screen.getAllByText("HRV", { selector: "dt" })).toHaveLength(1);
    expect(screen.getByText("2 sessions merged")).toBeInTheDocument();
    expect(screen.getByText("23:41 → 07:13")).toBeInTheDocument();
    expect(screen.getByText(/91% *efficiency/)).toBeInTheDocument();
    // SpO2 / skin / resp are null across all days: hidden
    expect(screen.queryByText("SpO₂")).toBeNull();
    expect(screen.queryByRole("radio", { name: "Skin temp" })).toBeNull();
    // trend with a null day is a gap and the caption summarises min/max
    const trends = screen.getByRole("region", { name: "14-day trends" });
    expect(within(trends).getByRole("img", { name: /Recovery, last 3 days: min 60%, max 74%, last 74%/ })).toBeInTheDocument();
    expect(within(trends).getByText(/Mean 67%/)).toBeInTheDocument();
    expect(container.textContent).not.toMatch(/whoop/i);
    expect(screen.getByText("NOOP band battery is 82%")).toBeInTheDocument();
  });

  it("waiting_for_nightly_data: effort shown, recovery/sleep pending", () => {
    render(<HealthField data={snap({ ...ready, status: "waiting_for_nightly_data", latestNight: null })} />);
    expect(screen.getByText(/Syncing overnight/)).toBeInTheDocument();
    expect(screen.getAllByText("Arrives after the sleep record closes")).toHaveLength(2);
    expect(screen.getByRole("meter", { name: "Effort" })).toBeInTheDocument();
    expect(screen.queryByRole("meter", { name: "Recovery" })).toBeNull();
  });

  it("stale: warning badge, last NOOP data date and sync hint", () => {
    render(<HealthField data={snap({ ...ready, status: "stale", dataAgeDays: 38, latestDay: "2026-09-20" })} />);
    const badge = screen.getByText("Stale");
    expect(badge.closest("[data-tone]")?.getAttribute("data-tone") ?? badge.getAttribute("data-tone")).toBe("warning");
    const note = screen.getByRole("note");
    expect(note).toHaveTextContent("Last NOOP data Sep 20.");
    expect(note).toHaveTextContent("Open NOOP to sync.");
    expect(screen.getByText("Updated 38 days ago at", { exact: false })).toBeInTheDocument();
  });

  it("ready data shows no stale note", () => {
    render(<HealthField data={snap(ready)} />);
    expect(screen.queryByText("Open NOOP to sync.")).not.toBeInTheDocument();
    expect(screen.queryByText("Stale")).not.toBeInTheDocument();
  });

  it("trend with fewer than 2 points says so", () => {
    render(<HealthField data={snap({ ...ready, recentDays: [day("2026-10-28")] })} />);
    expect(screen.getByText("Trend appears after 2+ days of data.")).toBeInTheDocument();
  });

  it("helpers: recovery bands and insight dedupe", () => {
    expect(recoveryBand(67)?.word).toBe("High");
    expect(recoveryBand(34)?.word).toBe("Medium");
    expect(recoveryBand(33)?.word).toBe("Low");
    expect(recoveryBand(null)).toBeNull();
    expect(signed(0)).toBe("0.0");
    expect(signed(-0.04)).toBe("0.0");
    expect(signed(0.3)).toBe("+0.3");
    expect(signed(-0.2)).toBe("−0.2");
    const merged = buildInsights({ ...(INITIAL_SNAPSHOT.noop), ...ready, insights: [{ metric: "latest_sleep", fact: "server", context: null }] } as NoopHealthSnapshot);
    expect(merged.filter((i) => i.metric === "latest_sleep")).toEqual([{ metric: "latest_sleep", fact: "server", context: null }]);
  });
});
