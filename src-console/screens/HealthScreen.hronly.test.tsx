import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { NoopDailyMetric, NoopHealthSnapshot, SystemSnapshot } from "../types";
import { HealthField } from "./HealthScreen";

// 0035: the Recovery card says why today has no score when Needs you holds `strap-hr-only`.
const needsYou = vi.fn();
vi.mock("../lib/localStores", async (orig) => ({ ...(await orig<typeof import("../lib/localStores")>()), inTauri: () => true, needsYouList: () => needsYou() }));

const day = (d: string, over: Partial<NoopDailyMetric> = {}): NoopDailyMetric => ({
  day: d, source: "noop", totalSleepMin: 452, efficiency: 0.91, deepMin: 92, remMin: 104, lightMin: 230,
  restingHr: 52, avgHrv: 64, recovery: 74, strain: 11.3, spo2Pct: null, skinTempDevC: null, respRateBpm: null, ...over,
});
const snap = (todayRecovery: number | null): SystemSnapshot => ({
  ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1,
  noop: {
    ...INITIAL_SNAPSHOT.noop, status: "ready", generatedAt: "2026-10-28T07:30:00+09:00", dataAgeDays: 0, deviceId: "noop-1", latestDay: "2026-10-28",
    today: day("2026-10-28", { recovery: todayRecovery }),
    // yesterday has a score: the card must never borrow it
    recentDays: [day("2026-10-26", { recovery: 60 }), day("2026-10-27", { recovery: 81 }), day("2026-10-28", { recovery: todayRecovery })],
    latestNight: null, latestSleep: null, battery: null, insights: [], message: null,
  } as NoopHealthSnapshot,
});
const strapRow = { source: "strap", id: "strap-hr-only", title: "Last night came through as heart rate only", due: null, reason: "x", deepLink: { id: "mindbody", view: "fitness" } };
const reports = (rows: unknown[]) => [{ source: "todos", state: "ok", rows: [] }, { source: "strap", state: "ok", rows }];
const REASON = "No score today. Last night came through as heart rate only, so recovery couldn't be scored.";

beforeEach(() => needsYou.mockReset());
afterEach(() => needsYou.mockReset());

describe("Fitness Recovery card empty reason (0035)", () => {
  it("HR-only pointer and no score: shows the reason and opens Settings › Devices", async () => {
    needsYou.mockResolvedValue(reports([strapRow]));
    const onOpen = vi.fn();
    render(<HealthField data={snap(null)} onOpen={onOpen} />);
    const card = screen.getByRole("region", { name: "Recovery" });
    expect(await within(card).findByText(REASON)).toBeInTheDocument();
    expect(within(card).queryByText("Arrives after the sleep record closes")).toBeNull();
    expect(within(card).queryByRole("meter")).toBeNull();
    fireEvent.click(within(card).getByRole("button", { name: "Open Settings, Devices, to check the strap sync" }));
    expect(onOpen).toHaveBeenCalledWith({ id: "settings", view: "devices" });
  });

  it("no pointer and no score: the usual pending copy, and never an older day's score", async () => {
    needsYou.mockResolvedValue(reports([]));
    render(<HealthField data={snap(null)} />);
    const card = screen.getByRole("region", { name: "Recovery" });
    await waitFor(() => expect(needsYou).toHaveBeenCalled());
    expect(within(card).getByText("Arrives after the sleep record closes")).toBeInTheDocument();
    expect(within(card).queryByText(REASON)).toBeNull();
    expect(within(card).queryByRole("meter")).toBeNull();
    expect(card.textContent).not.toMatch(/81%/);
  });

  it("a score is present: the ring shows and the pointer is not asked for", () => {
    needsYou.mockResolvedValue(reports([strapRow]));
    render(<HealthField data={snap(74)} />);
    const card = screen.getByRole("region", { name: "Recovery" });
    expect(within(card).getByRole("meter", { name: "Recovery" })).toHaveAttribute("aria-valuetext", "74%, High");
    expect(within(card).queryByText(REASON)).toBeNull();
    expect(needsYou).not.toHaveBeenCalled();
  });

  it("the Needs you read fails: the usual pending copy", async () => {
    needsYou.mockRejectedValue(new Error("boom"));
    render(<HealthField data={snap(null)} />);
    await waitFor(() => expect(needsYou).toHaveBeenCalled());
    expect(within(screen.getByRole("region", { name: "Recovery" })).getByText("Arrives after the sleep record closes")).toBeInTheDocument();
  });
});
