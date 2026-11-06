import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getLiveHeartRate } from "../bridge";
import { LIVE_HEART_RATE_POLL_MS, PLAYBACK_TICK_MS, beatSeconds, nextPlayhead, useLiveHeartRate } from "./useLiveHeartRate";

vi.mock("../bridge", () => ({ getLiveHeartRate: vi.fn() }));
const fetch = vi.mocked(getLiveHeartRate);
const flush = async () => {
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
  act(() => { vi.advanceTimersByTime(0); });
};
const setHidden = (hidden: boolean) => Object.defineProperty(document, "hidden", { configurable: true, value: hidden });
const feed = (end: number, count: number, bpm = (i: number) => 60 + i) => ({
  bpm: bpm(count - 1), at: end,
  trace: Array.from({ length: count }, (_, i) => ({ bpm: bpm(i), at: end - count + 1 + i })),
});

describe("useLiveHeartRate", () => {
  beforeEach(() => { vi.useFakeTimers(); fetch.mockReset(); setHidden(false); });
  afterEach(() => { vi.useRealTimers(); setHidden(false); });

  it("opens on the newest sample", async () => {
    fetch.mockResolvedValue(feed(100, 5));
    const { result } = renderHook(() => useLiveHeartRate());
    await flush();
    expect(result.current?.bpm).toBe(64);
    expect(result.current?.samples).toHaveLength(5);
  });

  it("plays a new batch through one second at a time", async () => {
    fetch.mockResolvedValueOnce(feed(100, 5)).mockResolvedValue(feed(110, 15));
    const { result } = renderHook(() => useLiveHeartRate());
    await flush();
    const start = result.current!.at;
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    act(() => { vi.advanceTimersByTime(PLAYBACK_TICK_MS * 3); });
    expect(result.current!.at).toBeGreaterThan(start);
    expect(result.current!.at).toBeLessThan(110);
  });

  it("clears when the reading goes stale or the call fails, then recovers", async () => {
    fetch.mockResolvedValueOnce(feed(100, 3)).mockResolvedValueOnce(null).mockResolvedValueOnce(feed(101, 3)).mockRejectedValueOnce(new Error("no"));
    const { result } = renderHook(() => useLiveHeartRate());
    await flush();
    expect(result.current).not.toBeNull();
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(result.current).toBeNull();
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(result.current).not.toBeNull();
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(result.current).toBeNull();
  });

  it("doesn't poll while the window is hidden, and stops on unmount", async () => {
    fetch.mockResolvedValue(feed(100, 3));
    setHidden(true);
    const { unmount } = renderHook(() => useLiveHeartRate());
    await flush();
    expect(fetch).not.toHaveBeenCalled();
    setHidden(false);
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(fetch).toHaveBeenCalledTimes(1);
    unmount();
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS * 3); });
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});

describe("nextPlayhead", () => {
  it("starts at the newest sample and steps one second", () => {
    expect(nextPlayhead(null, 100)).toBe(100);
    expect(nextPlayhead(90, 100)).toBe(91);
    expect(nextPlayhead(100, 100)).toBe(100);
  });
  it("skips ahead when it falls too far behind or the feed restarts", () => {
    expect(nextPlayhead(10, 100)).toBe(100);
    expect(nextPlayhead(120, 100)).toBe(100);
  });
});

describe("beatSeconds", () => {
  it("matches the heart rate and stays in a safe range", () => {
    expect(beatSeconds(60)).toBe(1);
    expect(beatSeconds(120)).toBe(0.5);
    expect(beatSeconds(1000)).toBe(0.25);
    expect(beatSeconds(5)).toBe(2);
  });
});
