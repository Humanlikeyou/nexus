import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getLiveHeartRate } from "../bridge";
import { LIVE_HEART_RATE_POLL_MS, beatSeconds, useLiveHeartRate } from "./useLiveHeartRate";

vi.mock("../bridge", () => ({ getLiveHeartRate: vi.fn() }));
const fetch = vi.mocked(getLiveHeartRate);
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });
const setHidden = (hidden: boolean) => Object.defineProperty(document, "hidden", { configurable: true, value: hidden });

describe("useLiveHeartRate", () => {
  beforeEach(() => { vi.useFakeTimers(); fetch.mockReset(); setHidden(false); });
  afterEach(() => { vi.useRealTimers(); setHidden(false); });

  it("updates on each poll", async () => {
    fetch.mockResolvedValueOnce({ bpm: 61, at: 1 }).mockResolvedValueOnce({ bpm: 72, at: 2 });
    const { result } = renderHook(() => useLiveHeartRate());
    await flush();
    expect(result.current).toBe(61);
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(result.current).toBe(72);
  });

  it("clears the reading when it goes stale or the call fails", async () => {
    fetch.mockResolvedValueOnce({ bpm: 61, at: 1 }).mockResolvedValueOnce(null).mockResolvedValueOnce({ bpm: 62, at: 2 }).mockRejectedValueOnce(new Error("no"));
    const { result } = renderHook(() => useLiveHeartRate());
    await flush();
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(result.current).toBeNull();
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(result.current).toBe(62);
    act(() => { vi.advanceTimersByTime(LIVE_HEART_RATE_POLL_MS); });
    await flush();
    expect(result.current).toBeNull();
  });

  it("doesn't poll while the window is hidden, and stops on unmount", async () => {
    fetch.mockResolvedValue({ bpm: 61, at: 1 });
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

describe("beatSeconds", () => {
  it("matches the heart rate and stays in a safe range", () => {
    expect(beatSeconds(60)).toBe(1);
    expect(beatSeconds(120)).toBe(0.5);
    expect(beatSeconds(1000)).toBe(0.25);
    expect(beatSeconds(5)).toBe(2);
  });
});
