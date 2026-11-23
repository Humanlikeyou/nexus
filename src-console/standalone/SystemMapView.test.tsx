import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { sample, UNWIRED, type Wired } from "../lib/sampleAdapters";
import { makeSampleSystemMap, type SystemMap, type SystemMapAdapter } from "./systemMap";
import { REFRESH_MS, SystemMap as SystemMapView, pathName, relativeTime, summarize } from "./SystemMapView";

const NOW = Date.parse("2026-11-12T10:03:00+09:00");
const base = (makeSampleSystemMap().map() as { value: SystemMap }).value;

function adapterOf(w: Wired<SystemMap>) {
  const refresh = vi.fn(async () => undefined);
  const a: SystemMapAdapter = { map: () => w, refresh };
  return { a, refresh };
}
/** Real IntersectionObserver reports once on observe; the stub does the same (on screen). */
const renderMap = (w: Wired<SystemMap> = sample(base)) => { const x = adapterOf(w); render(<SystemMapView adapter={x.a} now={() => NOW} />); act(() => ioCallback?.([{ isIntersecting: true }])); return x; };

let ioCallback: ((e: Partial<IntersectionObserverEntry>[]) => void) | null = null;
beforeEach(() => {
  vi.useFakeTimers();
  delete document.documentElement.dataset.reduceMotion;
  ioCallback = null;
  vi.stubGlobal("IntersectionObserver", class { constructor(cb: typeof ioCallback) { ioCallback = cb; } observe() {} disconnect() {} unobserve() {} });
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("system map helpers", () => {
  it("summarises broken, all-flowing and mixed maps", () => {
    expect(summarize(base)).toBe("1 broken: Crews");
    const allOk = { ...base, paths: base.paths.map((p) => ({ ...p, status: "ok" as const })) };
    expect(summarize(allOk)).toBe("All 16 paths flowing");
    const two = { ...base, paths: base.paths.map((p) => (p.id === "app-telegram" ? { ...p, status: "broken" as const } : p)) };
    expect(summarize(two)).toBe("2 broken: Crews, Telegram");
    const mixed = { ...base, paths: base.paths.map((p) => (p.status === "broken" ? { ...p, status: "ok" as const } : p)) };
    expect(summarize(mixed)).toBe("13 flowing, 1 off on purpose, 2 not checked");
  });
  it("names a path by its non-app end", () => {
    expect(pathName(base.paths.find((p) => p.id === "app-crew")!, base.nodes)).toBe("Crews");
    expect(pathName(base.paths.find((p) => p.id === "hermes-router")!, base.nodes)).toBe("Hermes (Chat) to Amara Router");
  });
  it("says relative times in plain words", () => {
    expect(relativeTime(null, NOW)).toBeNull();
    expect(relativeTime("2026-11-12T10:02:40+09:00", NOW)).toBe("just now");
    expect(relativeTime("2026-11-12T10:00:00+09:00", NOW)).toBe("3 min ago");
    expect(relativeTime("2026-11-12T07:00:00+09:00", NOW)).toBe("3 hours ago");
    expect(relativeTime("2026-11-11T09:00:00+09:00", NOW)).toBe("yesterday");
  });
});

describe("SystemMap", () => {
  it("unwired: says Needs wiring and invents nothing", () => {
    renderMap(UNWIRED);
    expect(document.querySelector(".data-tag")?.textContent).toMatch(/Needs wiring/);
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("sample: shows the Sample data tag, summary and a button per path with state in its label", () => {
    renderMap();
    expect(screen.getByText("Sample data")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe("1 broken: Crews");
    expect(screen.getByText(/^Checked /)).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /: (Flowing|Broken|Off on purpose|Not checked)$/ })).toHaveLength(16);
    expect(screen.getByRole("button", { name: "Crew runs and approvals: Broken" }).getAttribute("data-status")).toBe("broken");
    expect(screen.getByRole("button", { name: "Notifications to your phone: Off on purpose" }).getAttribute("data-status")).toBe("off");
  });

  it("live: no Sample data tag", () => {
    renderMap({ state: "live", value: base });
    expect(screen.queryByText("Sample data")).toBeNull();
  });

  it("blinks the broken dot normally and holds still when motion is reduced", () => {
    const { unmount } = render(<SystemMapView adapter={adapterOf(sample(base)).a} now={() => NOW} />);
    expect(screen.getByRole("button", { name: "Crew runs and approvals: Broken" }).getAttribute("data-motion")).toBe("blink");
    expect(screen.getByRole("button", { name: "Every model request: Flowing" }).hasAttribute("data-motion")).toBe(false);
    unmount();
    document.documentElement.dataset.reduceMotion = "true";
    render(<SystemMapView adapter={adapterOf(sample(base)).a} now={() => NOW} />);
    expect(screen.getByRole("button", { name: "Crew runs and approvals: Broken" }).getAttribute("data-motion")).toBe("still");
  });

  it("opens a plain-English panel with carries, last worked, reason and fix; Escape closes and returns focus", async () => {
    renderMap();
    const dot = screen.getByRole("button", { name: "Crew runs and approvals: Broken" });
    dot.focus();
    fireEvent.click(dot);
    const panel = screen.getByRole("group", { name: "Crew runs and approvals, details" });
    expect(within(panel).getByText("Broken")).toBeTruthy();
    expect(within(panel).getByText("Not seen working yet")).toBeTruthy();
    expect(within(panel).getByText("The crew service isn't answering")).toBeTruthy();
    expect(within(panel).getByText("Open Docker and start the stack")).toBeTruthy();
    expect(dot.getAttribute("aria-expanded")).toBe("true");
    fireEvent.keyDown(panel, { key: "Escape" });
    expect(screen.queryByRole("group", { name: /details/ })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Every model request: Flowing" }));
    expect(screen.getByText("3 min ago")).toBeTruthy();
    expect(screen.getByText("This path is working.")).toBeTruthy();
  });

  it("refreshes every 10 s while on screen, and not at all while hidden", async () => {
    const { refresh } = renderMap();
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(refresh).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(REFRESH_MS * 2); });
    expect(refresh).toHaveBeenCalledTimes(3);
    // scrolled off screen
    act(() => ioCallback?.([{ isIntersecting: false }]));
    refresh.mockClear();
    await act(async () => { await vi.advanceTimersByTimeAsync(REFRESH_MS * 5); });
    expect(refresh).not.toHaveBeenCalled();
    // back on screen: checks right away, then keeps going
    act(() => ioCallback?.([{ isIntersecting: true }]));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(refresh).toHaveBeenCalledTimes(1);
    // tab hidden
    refresh.mockClear();
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    act(() => { document.dispatchEvent(new Event("visibilitychange")); });
    await act(async () => { await vi.advanceTimersByTimeAsync(REFRESH_MS * 3); });
    expect(refresh).not.toHaveBeenCalled();
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });

  it("stops refreshing when the map unmounts", async () => {
    const { a, refresh } = adapterOf(sample(base));
    const { unmount } = render(<SystemMapView adapter={a} now={() => NOW} />);
    act(() => ioCallback?.([{ isIntersecting: true }]));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    unmount(); refresh.mockClear();
    await act(async () => { await vi.advanceTimersByTimeAsync(REFRESH_MS * 3); });
    expect(refresh).not.toHaveBeenCalled();
  });
});
