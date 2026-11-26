import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PROTOCOLS } from "../legacy-core/lib/neurofeedback/protocols";
import { TrendsCard, type TrainingTrends } from "./TrendsCard";

const trend = (over: Partial<TrainingTrends["protocols"][number]["features"][number]> = {}) => ({
  feature: "resting_level:Alpha", unit: "%", calculation: "Alpha relative band power in the session's own protocol baseline", n: 8, median: 31, robustSd: 2,
  earlierMedian: 28.75, laterMedian: 33.75, change: 5, changeInSpreads: 2.5, slopePerSession: 0.8, direction: "higher" as const, confidence: "low",
  uncertainty: "From 8 sessions. ... a measured change, not evidence of a lasting or cognitive change.", ...over });

describe("decision 0059: trends card", () => {
  it("shows earlier and later medians, direction, count, confidence, calculation and uncertainty", async () => {
    const t: TrainingTrends = { version: "trends-v1-0059", records: 10, eligible: 8, note: "Time in target is not trended.",
      protocols: [{ protocolId: "calm", method: "training-v2-0057", sessions: 8, first: "2026-11-01T10:00:00Z", last: "2026-11-20T10:00:00Z", sessionIds: [], features: [trend(), trend({ feature: "final_threshold_scale", unit: "x", earlierMedian: 1, laterMedian: 1.04, direction: "no_clear_change", calculation: "the reward threshold at the end" })] }] };
    render(<TrendsCard protocols={PROTOCOLS} load={async () => t} />);
    const table = await screen.findByRole("table", { name: "Calm & Relax trend measures" });
    const rows = within(table).getAllByRole("row");
    expect(rows[1]).toHaveTextContent("Alpha resting level28.8%33.8%higher in later sessions · 8 sessions · confidence low");
    expect(rows[2]).toHaveTextContent("x1.00x1.04no clear change");
    expect(screen.getByText(/8 of 10 recorded sessions are usable/)).toBeInTheDocument();
    expect(screen.getByText(/not evidence of a lasting or cognitive change/)).toBeInTheDocument();
    expect(screen.getByText("Time in target is not trended.")).toBeInTheDocument();
  });

  it("below five sessions it says there is no trend yet", async () => {
    const t: TrainingTrends = { version: "v", records: 3, eligible: 3, note: "n", protocols: [{ protocolId: "focus", method: "m", sessions: 3, first: "2026-11-01", last: "2026-11-03", sessionIds: [], features: [] }] };
    render(<TrendsCard protocols={PROTOCOLS} load={async () => t} />);
    expect(await screen.findByText(/Not enough sessions for a trend yet/)).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });
});
