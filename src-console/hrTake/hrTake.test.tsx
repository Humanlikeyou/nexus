import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Wired } from "../lib/sampleAdapters";

import {
  ageLabel, coveredMinutes, HR_TAKE_GAP_MS, hrMemoryLine, hrTemplate, hrTextOk, isHrStatus, liveHrTake, liveOf, sampleHrTake, sampleWindow,
  windowBlock, windowStats, type HrTake, type HrTakeAdapter, type HrTakeStatus,
} from "./adapters";
import { HrTakeCard } from "./HrTakeCard";
import { useHrTake } from "./useHrTake";

const T = new Date("2026-10-28T14:00:00+09:00").getTime();
const MIN = 60_000;
const CALM = [79, 81, 80, 78, 77, 76, 75, 74, 72, 71, 70, 69, 68, 67, 68];
const take = (at: number, bpm: number[] = CALM): HrTake => { const window = sampleWindow(at, bpm); return { id: `t${at}`, generatedAt: at, window, text: hrTemplate(window), memory: null, question: "q" }; };
const sampleOf = (v: HrTakeStatus): Wired<HrTakeStatus> => ({ state: "sample", value: v });

describe("design-1111u heart-rate window rules", () => {
  it("stats, coverage (capped at 15) and the 10-of-15 / 10-minute-stale rule", () => {
    const w = sampleWindow(T, CALM);
    expect(windowStats(w.perMinute)).toEqual({ avg: 74, min: 67, max: 81, first: 79, last: 68 });
    expect(coveredMinutes([...w.perMinute, { t: T + MIN, bpm: 70 }])).toBe(15);
    expect(windowBlock(w, T + 5 * MIN)).toBeNull();
    expect(windowBlock(w, T + 10 * MIN + 1)).toBe("source_down");
    expect(windowBlock({ ...w, lastSampleAt: null }, T)).toBe("source_down");
    const nine = sampleWindow(T, [70, 71, 72, 73, 74, 75, 76, 77, 78, null, null, null, null, null, null]);
    expect(nine.coveredMin).toBe(9);
    expect(windowBlock(nine, T)).toBe("not_enough");
    const ten = sampleWindow(T, [70, 71, 72, 73, 74, 75, 76, 77, 78, 79, null, null, null, null, null]);
    expect(windowBlock(ten, T)).toBeNull();
  });

  it("the template uses only the window's numbers and your usual, with no causal or medical words", () => {
    for (const bpm of [CALM, [70, 71, 73, 76, 80, 84, 86, 88, 87, 85, 84, 83, 82, 82, 80], [60, 61, 60, 59, 60, 61, 60, 60, 59, 60, 61, 60, 60, 61, 60]]) {
      const w = sampleWindow(T, bpm);
      const text = hrTemplate(w);
      expect(text.length).toBeGreaterThan(20);
      expect(hrTextOk(text, w)).toBe(true);
    }
    const w = sampleWindow(T, CALM);
    expect(hrTemplate(w)).toMatch(/eased from \*\*79\*\* to \*\*68\*\*.*inside your usual for this time of day \(\*\*62–74\*\*\)/);
    expect(hrTextOk("Your heart rate averaged 91 today.", w)).toBe(false);
    expect(hrTextOk("Coffee caused the climb to 81.", w)).toBe(false);
    expect(hrTextOk("At 2:00 PM you were at 68.", w)).toBe(true);
  });

  it("age is quiet: Just now, then 'as of 2:00 PM · 18 min ago'", () => {
    expect(ageLabel(T, T + 20_000)).toBe("Just now");
    expect(ageLabel(T, T + 18 * MIN)).toBe("as of 2:00 PM · 18 min ago");
    expect(ageLabel(T, T + 75 * MIN)).toBe("as of 2:00 PM · 1 h 15 min ago");
  });

  it("the memory line is a template over the stored previous take", () => {
    expect(hrMemoryLine(null)).toBeNull();
    expect(hrMemoryLine(take(T - 47 * MIN))).toBe("At **1:13 PM** you were at **68** and settling.");
  });

  it("backend answers are shape-checked; anything else is unwired", async () => {
    expect(isHrStatus([])).toBe(false);
    expect(isHrStatus(null)).toBe(false);
    expect(isHrStatus({ kind: "take", due: false, take: { id: "x" } })).toBe(false);
    expect(isHrStatus({ kind: "take", due: false, take: take(T) })).toBe(true);
    expect(await liveHrTake.status()).toEqual({ state: "unwired" });
    expect(await liveHrTake.refresh("open")).toEqual({ state: "unwired" });
    expect(liveOf([])).toEqual({ state: "unwired" });
    expect(liveOf(undefined)).toEqual({ state: "unwired" });
    expect(liveOf({ kind: "take", due: false, take: take(T) }).state).toBe("live");
  });
});

describe("design-1111u sample adapter mocks the backend's 30-minute rule", () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(T); });

  it("first open writes one take; within 30 min it stays; concurrent refreshes make one; 30+ min makes a new one with a memory line", async () => {
    let now = T; const a = sampleHrTake({ scenario: "first", delayMs: 10 }, () => now);
    const s0 = await a.status();
    expect(s0).toEqual({ state: "sample", value: { kind: "none_yet", due: true } });
    const p1 = a.refresh("open"); const p2 = a.refresh("focus");
    expect(p1).toBe(p2);
    await vi.advanceTimersByTimeAsync(10);
    const r1 = await p1;
    if (r1.state !== "sample" || r1.value.kind !== "take") throw new Error("expected a take");
    const first = r1.value.take;
    expect(first.window.coveredMin).toBe(15);
    now = T + 29 * MIN;
    const r2 = await a.refresh("focus");
    expect(r2.state === "sample" && r2.value.kind === "take" && r2.value.take.id).toBe(first.id);
    expect(r2.state === "sample" && r2.value.due).toBe(false);
    now = T + 30 * MIN;
    expect((await a.status()).state === "sample" && ((await a.status()) as { value: HrTakeStatus }).value.due).toBe(true);
    const p3 = a.refresh("navigate"); await vi.advanceTimersByTimeAsync(10); const r3 = await p3;
    if (r3.state !== "sample" || r3.value.kind !== "take") throw new Error("expected a take");
    expect(r3.value.take.id).not.toBe(first.id);
    expect(r3.value.take.memory).toMatch(/^At \*\*2:00 PM\*\* you were at \*\*68\*\*/);
    expect(now - first.generatedAt).toBeGreaterThanOrEqual(HR_TAKE_GAP_MS);
  });

  it("not enough heart rate and WHOOP down write nothing and keep the clock (still due next time)", async () => {
    for (const scenario of ["not_enough", "source_down"] as const) {
      const a = sampleHrTake({ scenario, delayMs: 5 }, () => T);
      const p = a.refresh("open"); await vi.advanceTimersByTimeAsync(5); const r = await p;
      if (r.state !== "sample") throw new Error("sample");
      expect(r.value.kind).toBe(scenario);
      expect(r.value.due).toBe(true);
      if (r.value.kind === "not_enough") expect(r.value.window.coveredMin).toBe(6);
      if (r.value.kind === "source_down") expect(T - (r.value.lastSampleAt ?? 0)).toBeGreaterThan(10 * MIN);
    }
  });
});

describe("design-1111u HrTakeCard states (no button, sample tagged)", () => {
  const cur = take(T);
  const cases: Array<[string, Wired<HrTakeStatus> | null, boolean, number, RegExp]> = [
    ["fresh", sampleOf({ kind: "take", take: cur, due: false }), false, T + 10_000, /Just now/],
    ["current", sampleOf({ kind: "take", take: cur, due: false }), false, T + 18 * MIN, /as of 2:00 PM · 18 min ago/],
    ["regenerating", sampleOf({ kind: "take", take: cur, due: true }), true, T + 47 * MIN, /Reading your heart rate from 2:32–2:47 PM…/],
    ["not-enough", sampleOf({ kind: "not_enough", previous: cur, window: sampleWindow(T + 47 * MIN, [72, 71, 73, null, null, null, null, null, null, null, null, null, 74, 75, 74]), due: true }), false, T + 47 * MIN, /Only 6 of the last 15 minutes have readings\. Amara writes once there are 10 or more\. The 2:00 PM take is kept\./],
    ["source-down", sampleOf({ kind: "source_down", previous: cur, lastSampleAt: T - 40 * MIN, due: true }), false, T + 47 * MIN, /WHOOP hasn't synced since 1:20 PM, so Amara won't write a take from old data/],
  ];
  for (const variant of ["landing", "inline"] as const) {
    for (const [state, w, reading, now, text] of cases) {
      it(`${variant}: ${state}`, () => {
        const { container } = render(<HrTakeCard w={w} reading={reading} now={now} variant={variant} onOpenDevices={() => undefined} />);
        expect(container.querySelector("[data-state]")).toHaveAttribute("data-state", state);
        expect(container.textContent).toMatch(text);
        expect(screen.getByText("Sample data")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: /refresh|fresh take|regenerate/i })).toBeNull();
        expect(container.textContent).not.toMatch(/NOOP|\d+:\d+ left|countdown/i);
      });
    }
  }
  it("fresh landing shows the numbers it used, the window line and the memory line; inline adds the 15-minute line", () => {
    const t = { ...cur, memory: "At **1:12 PM** you were at **84** and climbing." };
    const { container, rerender } = render(<HrTakeCard w={sampleOf({ kind: "take", take: t, due: false })} reading={false} now={T} variant="landing" />);
    expect(screen.getByRole("list", { name: "Numbers Amara used" })).toHaveTextContent("Avg74 bpmRange67–81Trend▼ 11");
    expect(container.textContent).toContain("From 1:45–2:00 PM · WHOOP · 15 of 15 min");
    expect(container.textContent).toContain("At 1:12 PM you were at 84 and climbing.");
    expect(screen.queryByRole("img", { name: /^Heart rate, last 15 minutes/ })).toBeNull();
    rerender(<HrTakeCard w={sampleOf({ kind: "take", take: t, due: false })} reading={false} now={T} variant="inline" />);
    expect(screen.getByRole("img", { name: "Heart rate, last 15 minutes: 79 to 68, average 74; your usual 62–74" })).toBeInTheDocument();
  });
  it("unwired: Needs wiring, no numbers; source down links to Settings › Devices", () => {
    const { container, rerender } = render(<HrTakeCard w={{ state: "unwired" }} reading={false} now={T} variant="landing" />);
    expect(screen.getByText("Needs wiring")).toBeInTheDocument();
    expect(container.textContent?.replace(/15 minutes|30 minutes/g, "")).not.toMatch(/\d/);
    const open = vi.fn();
    rerender(<HrTakeCard w={sampleOf({ kind: "source_down", previous: null, lastSampleAt: null, due: true })} reading={false} now={T} variant="landing" onOpenDevices={open} />);
    fireEvent.click(screen.getByRole("button", { name: "Check WHOOP in Settings › Devices ›" }));
    expect(open).toHaveBeenCalled();
  });
});

function Probe({ adapter }: { adapter: HrTakeAdapter }) {
  const { w, reading } = useHrTake("open", adapter);
  return <p data-testid="p">{w == null ? "none" : w.state}{reading ? " reading" : ""}</p>;
}
describe("design-1111u triggers: open, focus and navigation; the UI never decides", () => {
  it("asks on open and on focus, and refreshes only when the backend says a take is due", async () => {
    const status = vi.fn(async (): Promise<Wired<HrTakeStatus>> => sampleOf({ kind: "take", take: take(T), due: false }));
    const refresh = vi.fn(async (): Promise<Wired<HrTakeStatus>> => sampleOf({ kind: "take", take: take(T), due: false }));
    const adapter = { status, refresh };
    render(<Probe adapter={adapter} />);
    await act(async () => {});
    expect(status).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
    status.mockResolvedValueOnce(sampleOf({ kind: "take", take: take(T), due: true }));
    await act(async () => { window.dispatchEvent(new Event("focus")); });
    await act(async () => {});
    expect(status).toHaveBeenCalledTimes(2);
    expect(refresh).toHaveBeenCalledWith("focus");
    expect(screen.getByTestId("p")).toHaveTextContent("sample");
  });
});
