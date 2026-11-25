import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionWithSamples } from "./sessionSamples";

const ipc = vi.hoisted(() => ({ getSessionSamples: vi.fn() }));
vi.mock("../bridge", () => ipc);
vi.mock("../lib/localStores", () => ({ inTauri: () => true }));

import { samplesFromIpc, useSessionBands } from "./useSessionBands";

const series = (n: number) => ({ stepSecs: 5, bands: { delta: Array(n).fill(20), theta: Array(n).fill(15), alpha: Array(n).fill(30), beta: Array(n).fill(25), gamma: Array(n).fill(10) }, focus: Array(n).fill(60), calm: Array(n).fill(70) });
const sess = (id: string, samples?: SessionWithSamples["samples"]) => ({ id, samples } as unknown as SessionWithSamples);

describe("band series for the newest sessions (decision 0045)", () => {
  beforeEach(() => { ipc.getSessionSamples.mockReset(); });

  it("drops gaps and returns nothing to draw for an empty or too-short series", () => {
    const r = samplesFromIpc({ ...series(4), bands: { ...series(4).bands, alpha: [30, null, 31, 29] } } as never);
    expect(r?.bands.alpha).toEqual([30, 31, 29]);
    expect(samplesFromIpc(null)).toBeNull();
    expect(samplesFromIpc(series(1) as never)).toBeNull();
    expect(samplesFromIpc({ ...series(4), stepSecs: 0 } as never)).toBeNull();
  });

  it("fetches one session at a time, newest first, and merges what it gets", async () => {
    ipc.getSessionSamples.mockImplementation(async (id: string) => series(id === "s-new" ? 6 : 8));
    const { result } = renderHook(() => useSessionBands([sess("s-new"), sess("s-old")]));
    await waitFor(() => expect(result.current[0].samples?.bands.alpha).toHaveLength(6));
    await waitFor(() => expect(result.current[1].samples?.bands.alpha).toHaveLength(8));
    expect(ipc.getSessionSamples.mock.calls.map((c) => c[0])).toEqual(["s-new", "s-old"]);
  });

  it("skips sessions that already carry samples and sessions with no replay, without retrying", async () => {
    ipc.getSessionSamples.mockResolvedValue(null);
    const has = sess("have", samplesFromIpc(series(5) as never));
    const { result } = renderHook(() => useSessionBands([has, sess("none")]));
    await waitFor(() => expect(ipc.getSessionSamples).toHaveBeenCalledTimes(1));
    expect(ipc.getSessionSamples).toHaveBeenCalledWith("none");
    await new Promise((r) => setTimeout(r, 30));
    expect(ipc.getSessionSamples).toHaveBeenCalledTimes(1);
    expect(result.current[1].samples ?? null).toBeNull();
  });

  it("a failing read leaves the session as it was and does not loop", async () => {
    ipc.getSessionSamples.mockRejectedValue(new Error("Recording not found"));
    const { result } = renderHook(() => useSessionBands([sess("bad")]));
    await waitFor(() => expect(ipc.getSessionSamples).toHaveBeenCalledTimes(1));
    await new Promise((r) => setTimeout(r, 30));
    expect(ipc.getSessionSamples).toHaveBeenCalledTimes(1);
    expect(result.current[0].samples ?? null).toBeNull();
  });
});
