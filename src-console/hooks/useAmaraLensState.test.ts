import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("../bridge", () => ({ invokeBound: vi.fn(async () => true) }));

import { ingestLevel, ingestState } from "./useAmaraVoice";
import { settledLensState, useAmaraLensState, voiceLoudness, WAKE_MS } from "./useAmaraLensState";

describe("settledLensState", () => {
  it("maps the voice state to off / listening / speaking", () => {
    expect(settledLensState("idle")).toBe("off");
    expect(settledLensState("listening")).toBe("listening");
    expect(settledLensState("thinking")).toBe("listening");
    expect(settledLensState("speaking")).toBe("speaking");
    expect(settledLensState("error", "Out of reach")).toBe("off");
    expect(settledLensState("error", "tts hiccup")).toBe("listening");
  });
});

describe("voiceLoudness (AnalyserNode stand-in)", () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date("2026-11-19T10:00:00Z")); });
  afterEach(() => { ingestState({ state: "idle", ts: Date.now() }); vi.useRealTimers(); });
  it("scales rms, clamps to 0..1 and is 0 once the feed is silent for 2 s", () => {
    ingestLevel({ ts: Date.now(), rms: 0.1, peak: 0.2, state: "speaking" });
    expect(voiceLoudness()).toBeCloseTo(0.32, 5);
    ingestLevel({ ts: Date.now(), rms: 0.9, peak: 1, state: "speaking" });
    expect(voiceLoudness()).toBe(1);
    vi.advanceTimersByTime(2100);
    expect(voiceLoudness()).toBe(0);
  });
});

describe("useAmaraLensState", () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date("2026-11-19T10:00:00Z")); });
  afterEach(() => { ingestState({ state: "idle", ts: Date.now() }); vi.useRealTimers(); });
  it("goes off -> wake -> listening -> speaking -> off", () => {
    const { result } = renderHook(() => useAmaraLensState());
    expect(result.current.state).toBe("off");
    act(() => ingestState({ state: "listening", ts: Date.now() }));
    expect(result.current.state).toBe("wake");
    act(() => { vi.advanceTimersByTime(WAKE_MS + 10); });
    expect(result.current.state).toBe("listening");
    act(() => ingestState({ state: "speaking", ts: Date.now() }));
    expect(result.current.state).toBe("speaking");
    act(() => ingestState({ state: "idle", ts: Date.now() }));
    expect(result.current.state).toBe("off");
  });
  it("a wake that is cut short by going off never lingers", () => {
    const { result } = renderHook(() => useAmaraLensState());
    act(() => ingestState({ state: "listening", ts: Date.now() }));
    act(() => ingestState({ state: "idle", ts: Date.now() }));
    expect(result.current.state).toBe("off");
  });
});
