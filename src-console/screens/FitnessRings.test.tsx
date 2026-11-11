import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { NoopDailyMetric } from "../types";
import { EffortCard, RecoveryCard } from "./FitnessRings";
import { effortModel, effortTakeaway, recoveryModel, recoveryTakeaway, usualOf } from "./fitnessRingsModel";

/** design-1111r B "Horizon": Recovery (no Sleep tile) and Effort, every state. */
const RECOVERY = [62, 70, 58, 66, 81, 77, 54, 49, 68, 72, 80, 61, 66, 74];
const day = (i: number, o: Partial<NoopDailyMetric> = {}): NoopDailyMetric => ({
  day: new Date(Date.parse("2026-10-15T00:00:00Z") + i * 864e5).toISOString().slice(0, 10), source: "noop",
  totalSleepMin: 452, efficiency: 0.91, deepMin: 92, remMin: 104, lightMin: 230, restingHr: 50 + (i % 4), avgHrv: 55 + ((i * 7) % 14),
  recovery: RECOVERY[i], strain: 12 + ((i * 11) % 40), spo2Pct: null, skinTempDevC: null, respRateBpm: null, ...o,
});
const DAYS = RECOVERY.map((_, i) => day(i));
const TODAY = DAYS[13].day;
const clamp = (v: number) => Math.min(100, Math.max(0, v));
const rec = (o: Partial<Parameters<typeof recoveryModel>[0]> = {}, days = DAYS) => recoveryModel({ day: TODAY, recovery: 74, hrv: 66, rhr: 51, nightly: false, ...o }, days);
const eff = (o: Partial<Parameters<typeof effortModel>[0]> = {}, days = DAYS) => effortModel({ day: TODAY, effort: 35, sampleZones: false, ...o }, days, clamp);

describe("fitness rings model", () => {
  it("usual is the mean of the previous 13 readings, only from 7", () => {
    expect(usualOf(DAYS, TODAY, (d) => d.recovery).usual).toBeCloseTo(864 / 13, 5);
    expect(usualOf(DAYS.slice(7), TODAY, (d) => d.recovery)).toEqual({ usual: null, count: 6 });
  });
  it("live recovery: two drivers only (HRV, resting HR), week ends today", () => {
    const m = rec();
    expect(m.state).toBe("live");
    expect(m.drivers.map((d) => d.key)).toEqual(["hrv", "rhr"]);
    expect(m.week).toHaveLength(7);
    expect(m.week[6]).toEqual({ day: TODAY, value: 74 });
    expect(m.band).toBe("High");
  });
  it("building, pending and unwired", () => {
    expect(rec({}, DAYS.slice(10)).state).toBe("building");
    const p = rec({ nightly: true });
    expect(p.state).toBe("pending");
    expect(p.week).toHaveLength(6);
    expect(p.week[p.week.length - 1].day).toBe(DAYS[12].day);
    expect(rec({ recovery: null }, DAYS.map((d) => ({ ...d, recovery: null }))).state).toBe("unwired");
    expect(eff({ effort: null }, DAYS.map((d) => ({ ...d, strain: null }))).state).toBe("unwired");
  });
  it("takeaways only say what the card shows", () => {
    const drivers = rec().drivers;
    expect(recoveryTakeaway(74, 66.5, "High", drivers)).toBe("Above your usual, led by HRV. Room for a harder session today.");
    expect(recoveryTakeaway(66, 66.5, "Medium", drivers)).toBe("Close to your usual.");
    expect(recoveryTakeaway(74, null, "High", drivers)).toMatch(/after 7 days/);
    expect(effortTakeaway(35, 29, [{ day: "a", value: 20 }, { day: "b", value: 49 }, { day: "c", value: 35 }])).toBe("Above your usual so far today; inside this week's range.");
  });
  it("zone minutes are Needs wiring unless sample adapters are on", () => {
    expect(eff().zones.state).toBe("unwired");
    expect(eff({ sampleZones: true }).zones.state).toBe("sample");
  });
});

describe("RecoveryCard / EffortCard", () => {
  it("Recovery live: number, usual, meter, HRV and resting HR tiles, no Sleep tile", () => {
    render(<RecoveryCard m={rec()} />);
    const card = screen.getByRole("region", { name: "Recovery" });
    expect(within(card).getByRole("meter", { name: "Recovery" })).toHaveAttribute("aria-valuetext", "74%, High");
    expect(within(card).getByText(/vs usual 66%/)).toBeInTheDocument();
    const tiles = within(card).getByRole("list", { name: "What moved recovery" });
    expect(within(tiles).getAllByRole("listitem")).toHaveLength(2);
    expect(tiles).toHaveTextContent("HRV");
    expect(tiles).toHaveTextContent("Rest HR");
    expect(card.textContent).not.toMatch(/sleep/i);
    expect(within(card).getByRole("img", { name: /^Recovery, last 7 days: .* Oct 28 74%; usual 66%$/ })).toBeInTheDocument();
    expect(card.textContent).not.toContain("Sample data");
  });
  it("Recovery building / pending / unwired", () => {
    const { rerender } = render(<RecoveryCard m={rec({}, DAYS.slice(10))} />);
    expect(screen.getByText("usual after 7 days (3 of 7)")).toBeInTheDocument();
    rerender(<RecoveryCard m={rec({ nightly: true })} />);
    expect(screen.getByText("Arrives after the sleep record closes")).toBeInTheDocument();
    expect(screen.queryByRole("meter", { name: "Recovery" })).toBeNull();
    rerender(<RecoveryCard m={rec({ recovery: null }, [])} />);
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThan(0);
    expect(screen.queryByText(/\d+%/)).toBeNull();
  });
  it("Effort live: zone minutes say Needs wiring, never numbers", () => {
    render(<EffortCard m={eff()} max={100} />);
    const card = screen.getByRole("region", { name: "Effort" });
    expect(within(card).getByRole("meter", { name: "Effort" })).toHaveAttribute("aria-valuenow", "35");
    expect(card).toHaveTextContent("Zone minutes");
    expect(card).toHaveTextContent("Needs wiring");
    expect(card.textContent).not.toMatch(/Z[1-5]/);
    expect(card.textContent).not.toContain("Sample data");
  });
  it("Effort with sample adapters: zone minutes tagged Sample data", () => {
    render(<EffortCard m={eff({ sampleZones: true })} max={100} />);
    expect(screen.getByText("Sample data")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Z2 31 minutes/ })).toBeInTheDocument();
  });
});
