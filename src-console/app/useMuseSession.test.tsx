import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({ disconnectMuse: vi.fn(), startEegRecording: vi.fn(), startEegStream: vi.fn(), stopEegStream: vi.fn(), stopEegRecording: vi.fn(), saveMuseSession: vi.fn() }));
vi.mock("../bridge", async (orig) => ({ ...(await orig<typeof import("../bridge")>()), ...bridge }));
import { INITIAL_SNAPSHOT } from "../bridge";
import { DISCONNECT_TIMEOUT_MS, SAVE_TIMEOUT_MS, useMuseSession } from "./useMuseSession";

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

describe("Disconnect", () => {
  it("times out with a clear message and leaves the screen usable when the headset never answers", async () => {
    bridge.disconnectMuse.mockReturnValue(new Promise(() => undefined));
    const refresh = vi.fn(async () => undefined);
    const { result } = renderHook(() => useMuseSession(INITIAL_SNAPSHOT, refresh, vi.fn()));
    act(() => { void result.current.disconnectFromMuse(); });
    expect(result.current.museOperation).toBe("disconnecting");
    await act(async () => { await vi.advanceTimersByTimeAsync(DISCONNECT_TIMEOUT_MS + 10); });
    expect(result.current.museOperation).toBeNull();
    expect(result.current.museError).toMatch(/didn't answer the disconnect/);
  });
});

describe("Stop", () => {
  it("times out when the headset hangs, and a retry joins the same save without a second one", async () => {
    let releaseStop: () => void = () => undefined;
    bridge.startEegRecording.mockResolvedValue("/tmp/eeg_test.csv");
    bridge.startEegStream.mockResolvedValue(undefined);
    bridge.stopEegStream.mockReturnValueOnce(new Promise<void>((r) => { releaseStop = r; }));
    bridge.stopEegRecording.mockResolvedValue("/tmp/eeg_test.csv");
    bridge.saveMuseSession.mockResolvedValue(undefined);
    const refresh = vi.fn(async () => undefined);
    const { result } = renderHook(() => useMuseSession(INITIAL_SNAPSHOT, refresh, vi.fn()));
    await act(async () => { await result.current.startMuseSession(); });
    expect(result.current.sessionStartedAt).not.toBeNull();

    act(() => { void result.current.stopMuseSession(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(SAVE_TIMEOUT_MS + 10); });
    expect(result.current.museOperation).toBeNull();
    expect(result.current.museError).toMatch(/didn't answer while saving/);
    expect(result.current.sessionStartedAt).not.toBeNull();
    expect(bridge.saveMuseSession).not.toHaveBeenCalled();

    // Retry while the first stop is still in flight: it joins it, no second stopEegStream.
    act(() => { void result.current.stopMuseSession(); });
    await act(async () => { releaseStop(); await vi.advanceTimersByTimeAsync(10); });
    expect(bridge.stopEegStream).toHaveBeenCalledTimes(1);
    expect(bridge.saveMuseSession).toHaveBeenCalledTimes(1);
    expect(result.current.sessionStartedAt).toBeNull();
  });
});

describe("Disconnect superseded", () => {
  it("never shows the raw superseded text", async () => {
    bridge.disconnectMuse.mockRejectedValue("superseded");
    const { result } = renderHook(() => useMuseSession(INITIAL_SNAPSHOT, vi.fn(async () => undefined), vi.fn()));
    await act(async () => { await result.current.disconnectFromMuse(); });
    expect(result.current.museError).toBe("The headset didn't answer; try again");
  });
});
