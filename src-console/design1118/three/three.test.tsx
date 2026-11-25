import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HrWindow } from "../../hrTake/adapters";
import type { RecordedSession } from "../../types";
import { hrFacts, hrShape } from "./hrData";
import { bandShares, percentile, placeOf, sectorAngles, sessionShares, usualBandRanges, usualOf, usualSummary } from "./neuroData";
import { NeuroLiveRing } from "./NeuroLiveRing";
import { BrainText } from "./NeuroBrainRing";
import { bandRows } from "./neuroData";
import { HrTrace } from "../../hrTake/HrTakeCard";
import { MuseJourney } from "../MuseJourney";

const mk = (id: string, day: number, bands?: number[], focus = 60): RecordedSession => ({
  id, device_type: "muse", start: new Date(2026, 10, day, 9).toISOString(), end: new Date(2026, 10, day, 9, 20).toISOString(), duration_secs: 1200, data_points: 10,
  summary: { avg_focus: focus, avg_relaxation: 50 },
  ...(bands ? { samples: { stepSecs: 5, focus: [1, 2], calm: [1, 2], bands: { delta: [bands[0], bands[0]], theta: [bands[1], bands[1]], alpha: [bands[2], bands[2]], beta: [bands[3], bands[3]], gamma: [bands[4], bands[4]] } } } : {}),
} as RecordedSession);

describe("band shares and usual range (pure)", () => {
  it("normalises five bands to 100 and refuses missing or negative data", () => {
    const s = bandShares({ delta: 10, theta: 10, alpha: 20, beta: 40, gamma: 20 })!;
    expect(s.beta).toBe(40); expect(Object.values(s).reduce((a, b) => a + b, 0)).toBeCloseTo(100, 0);
    expect(bandShares({ delta: 1 })).toBeNull(); expect(bandShares(null)).toBeNull();
    expect(bandShares({ delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 })).toBeNull();
    expect(bandShares({ delta: -1, theta: 1, alpha: 1, beta: 1, gamma: 1 })).toBeNull();
  });
  it("percentile and usualOf need at least 3 values", () => {
    expect(percentile([1, 2, 3, 4, 5], 0.25)).toBe(2);
    expect(usualOf([1, 2])).toBeNull();
    expect(usualOf([10, 20, 30, 40, 50, null, NaN])).toEqual({ lo: 20, hi: 40 });
  });
  it("usual ranges: skips sessions without band data and the shown session, needs 3, caps at 30", () => {
    const withBands = [1, 2, 3, 4].map((d) => mk(`s${d}`, d, [10 + d, 10, 30, 30, 20]));
    const none = mk("flat", 5);
    expect(usualBandRanges([none, ...withBands.slice(0, 2)]).ranges).toBeNull();
    expect(usualBandRanges([none, ...withBands.slice(0, 2)]).count).toBe(2);
    expect(usualBandRanges(withBands, "s4").count).toBe(3);
    const many = Array.from({ length: 40 }, (_, i) => mk(`m${i}`, (i % 27) + 1, [10, 10, 30, 30, 20]));
    expect(usualBandRanges(many).count).toBe(30);
    expect(usualBandRanges(withBands).ranges).not.toBeNull();
  });
  it("session without samples has no shares; place words", () => {
    expect(sessionShares(mk("a", 1))).toBeNull(); expect(sessionShares(null)).toBeNull();
    expect(placeOf(30, { lo: 10, hi: 20 })).toBe("above"); expect(placeOf(5, { lo: 10, hi: 20 })).toBe("below"); expect(placeOf(15, { lo: 10, hi: 20 })).toBe("within"); expect(placeOf(5, null)).toBeNull();
  });
  it("usual focus needs 3 other sessions", () => {
    const ss = [1, 2, 3, 4].map((d) => mk(`f${d}`, d, undefined, 50 + d * 5));
    expect(usualSummary(ss.slice(0, 2), (s) => s.summary.avg_focus)).toBeNull();
    expect(usualSummary(ss, (s) => s.summary.avg_focus, "f4")).toEqual({ lo: 57.5, hi: 62.5 });
  });
  it("sector angles run clockwise from the top without overlapping", () => {
    for (let i = 0; i < 4; i++) expect(sectorAngles(i).a0).toBeGreaterThan(sectorAngles(i + 1).a1);
  });
});

const win = (bpms: Array<number | null>, usual: { lo: number; hi: number; days: number } | null = { lo: 62, hi: 78, days: 14 }): HrWindow => ({
  from: 0, to: bpms.length * 60_000, source: "whoop", lastSampleAt: null, coveredMin: bpms.filter((b) => b != null).length, stats: null, usual,
  perMinute: bpms.map((bpm, i) => ({ t: i * 60_000, bpm })),
});

describe("HR take geometry (pure)", () => {
  it("splits runs at gaps and never joins across them", () => {
    const s = hrShape(win([70, 71, null, null, 80, 81]));
    expect(s.runs.map((r) => r.length)).toEqual([2, 2]);
    expect(s.gaps).toEqual([{ from: 1, to: 4, minutes: 2 }]);
    expect(s.last).toEqual({ i: 5, bpm: 81 });
    expect(hrFacts(win([70, null, 80]), hrShape(win([70, null, 80]))).join(" ")).toMatch(/No signal between/);
  });
  it("no readings: no runs, honest sentence", () => {
    const w = win([null, null], null);
    expect(hrShape(w).runs).toEqual([]); expect(hrFacts(w, hrShape(w))[0]).toMatch(/No heart-rate readings/);
  });
});

describe("fallbacks without WebGL (jsdom)", () => {
  it("live ring is hidden with the small note when Muse is not connected", () => {
    render(<NeuroLiveRing bands={null} ranges={null} onFail={() => undefined} />);
    expect(screen.getByText("Shows while Muse is connected")).toBeInTheDocument();
    expect(document.querySelector("canvas")).toBeNull();
  });
  it("live ring lists live shares as text when streaming", () => {
    render(<NeuroLiveRing bands={{ delta: 10, theta: 10, alpha: 40, beta: 30, gamma: 10 }} ranges={null} onFail={() => undefined} />);
    expect(screen.getByLabelText("Live brainwave bands")).toHaveTextContent("Alpha 40%");
  });
  it("HR trace keeps its SVG and label", () => {
    render(<HrTrace w={{ ...win([70, null, 72]), stats: { avg: 71, min: 70, max: 72, first: 70, last: 72 } }} />);
    expect(screen.getByRole("img", { name: /Heart rate, last 15 minutes: 70 to 72/ })).toBeInTheDocument();
    expect(document.querySelector("canvas")).toBeNull();
  });
  it("brain text alternative states the shares and the usual range, or why there is none", () => {
    const shares = bandShares({ delta: 10, theta: 10, alpha: 40, beta: 30, gamma: 10 })!;
    const { unmount } = render(<BrainText rows={bandRows(shares, null)} usualCount={1} centre={{ date: "Wed 18 Nov", duration: "24 min" }} />);
    expect(screen.getByRole("table")).toHaveTextContent("Alpha");
    expect(screen.getByText(/appears once 3 sessions have band data \(1 so far\)/)).toBeInTheDocument();
    unmount();
  });
  it("MuseJourney: no band data gives the honest line and the old rings", () => {
    render(<MuseJourney session={mk("x", 3)} sessions={[mk("x", 3)]} recoveryNow={null} action={null} />);
    expect(screen.getByTestId("no-band-data")).toHaveTextContent(/only focus and calm/);
    expect(screen.getByLabelText(/Focus 60/)).toBeInTheDocument();
  });
  it("MuseJourney: with band data but no WebGL it still falls back to the old rings", () => {
    const s = mk("y", 4, [10, 10, 40, 30, 10]);
    render(<MuseJourney session={s} sessions={[s]} recoveryNow={null} action={null} />);
    expect(screen.queryByTestId("neuro-brain-ring")).toBeNull();
    expect(screen.queryByTestId("no-band-data")).toBeNull();
  });
});

describe("WebGL on but three fails to load", () => {
  it("falls back to the flat rendering", async () => {
    vi.resetModules();
    vi.doMock("./stage", async (orig) => ({ ...(await orig<typeof import("./stage")>()), glOk: () => true }));
    vi.doMock("three", () => { throw new Error("no three"); });
    const { MuseJourney: MJ } = await import("../MuseJourney");
    const s = mk("z", 5, [10, 10, 40, 30, 10]);
    const { findByLabelText } = render(<MJ session={s} sessions={[s]} recoveryNow={null} action={null} />);
    expect(await findByLabelText(/Focus 60/)).toBeInTheDocument();
    vi.doUnmock("./stage"); vi.doUnmock("three");
  });
});
