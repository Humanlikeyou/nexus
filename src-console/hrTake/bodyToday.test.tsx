import { act, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import { TerranceField } from "../screens/TerranceScreen";
import type { NoopDailyMetric, SystemSnapshot } from "../types";

const NOW = new Date("2026-10-28T14:00:00+09:00").getTime();
const day = (d: string, sleep: number, hrv: number, rec: number): NoopDailyMetric => ({ day: d, source: "noop", totalSleepMin: sleep, efficiency: 0.9, deepMin: 90, remMin: 100, lightMin: 230, restingHr: 52, avgHrv: hrv, recovery: rec, strain: 9, spo2Pct: null, skinTempDevC: null, respRateBpm: null });
const today = day("2026-10-28", 400, 38, 73);
const recent = ["21", "22", "23", "24", "25", "26", "27"].map((d) => day(`2026-10-${d}`, 440, 52, 66));
const data: SystemSnapshot = { ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, noop: { ...INITIAL_SNAPSHOT.noop, status: "ready", today, recentDays: [...recent, today] } };
const scheduler = { running: true, lastConsolidation: NOW - 3_600_000, lastReasoning: NOW - 7_200_000 };

describe("design-1111u C: Your body today", () => {
  it("one card: recovery ring, sleep and HRV once each with vs usual from earlier days (the heart-rate take is off Home, 2026-11-12)", async () => {
    render(<TerranceField data={data} now={NOW} scheduler={scheduler} onNavigate={() => undefined} />);
    await act(async () => {});
    const card = screen.getByRole("region", { name: "Your body today" });
    expect(within(card).getByRole("meter", { name: "Recovery" })).toHaveAttribute("aria-valuenow", "73");
    expect(within(card).getByText("High")).toHaveAttribute("data-band", "high");
    expect(card).toHaveTextContent("▲ 7 vs usual");
    expect(within(card).getByLabelText("Last night")).toHaveTextContent("Sleep6h 40m▼ 40 min vs usualHRV38 ms▼ 14 ms vs usual");
    // Taisun removed the heart-rate take from this card on 2026-11-12; it must not render here.
    expect(within(card).queryByRole("region", { name: "Amara's take" })).toBeNull();
    expect(within(card).getByRole("button", { name: "Open Fitness" })).toBeInTheDocument();
    expect(screen.queryByText("Recovery & sleep")).toBeNull();
    // Each number appears once on the card.
    expect(card.textContent?.match(/6h 40m/g)).toHaveLength(1);
    expect(card.textContent?.match(/38 ms/g)).toHaveLength(1);
  });
});
