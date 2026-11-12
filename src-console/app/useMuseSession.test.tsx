import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({ disconnectMuse: vi.fn() }));
vi.mock("../bridge", async (orig) => ({ ...(await orig<typeof import("../bridge")>()), disconnectMuse: bridge.disconnectMuse }));
import { INITIAL_SNAPSHOT } from "../bridge";
import { DISCONNECT_TIMEOUT_MS, useMuseSession } from "./useMuseSession";

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
