import { Profiler } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const events = vi.hoisted(() => ({
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
  unlisten: vi.fn(),
}));
const core = vi.hoisted(() => ({ invoke: vi.fn(async () => true) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: core.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, h: (e: { payload: unknown }) => void) => {
    events.handlers.set(name, h);
    return () => { events.unlisten(name); events.handlers.delete(name); };
  }),
}));

import { AMARA_VOICE_LEVEL_EVENT, AMARA_VOICE_STATE_EVENT, LEVEL_SILENCE_MS, STATE_FALLBACK_MS, __resetAmaraVoiceForTests, amaraVoiceLevel, ingestState, useAmaraVoice } from "../../hooks/useAmaraVoice";
import { AmaraLens, AmaraVoiceAnnouncer, LENS_ERROR_COPY, lensScale, stepEnvelope } from "./AmaraLens";

const emit = (name: string, payload: Record<string, unknown>) => act(() => { events.handlers.get(name)?.({ payload: { ts: Date.now(), ...payload } }); });
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

beforeEach(() => { events.handlers.clear(); events.unlisten.mockClear(); core.invoke.mockClear(); });
afterEach(() => { cleanup(); vi.useRealTimers(); __resetAmaraVoiceForTests(); });

describe("envelope", () => {
  it("attack is faster than release; scale never exceeds 1.12", () => {
    const up = stepEnvelope(0, 1, 40);
    const down = 1 - stepEnvelope(1, 0, 40);
    expect(up).toBeCloseTo(1 - Math.exp(-1), 5);
    expect(down).toBeCloseTo(1 - Math.exp(-0.25), 5);
    expect(up).toBeGreaterThan(down);
    expect(lensScale(0)).toBe(1);
    expect(lensScale(5)).toBeCloseTo(1.12, 10);
    expect(stepEnvelope(0, NaN, 16)).toBe(0);
  });
});

describe("AmaraLens", () => {
  it.each([["idle", "Amara is idle"], ["listening", "Amara is listening"], ["thinking", "Amara is thinking"], ["speaking", "Amara is speaking"], ["error", "Amara can't speak right now"]] as const)("%s: role img with its label", (state, label) => {
    render(<AmaraLens state={state} size={160} level={0} />);
    const img = screen.getByRole("img", { name: label });
    expect(img).toHaveAttribute("data-state", state);
  });

  it("error shows the fixed copy and optional Retry", () => {
    const onRetry = vi.fn();
    render(<AmaraLens state="error" size={160} showLabel onRetry={onRetry} />);
    expect(screen.getByText(LENS_ERROR_COPY)).toBeInTheDocument();
    act(() => screen.getByRole("button", { name: "Retry" }).click());
    expect(onRetry).toHaveBeenCalled();
  });

  it("28px drops halo and catchlight; decorative hides from AT", () => {
    const { container } = render(<AmaraLens state="speaking" size={28} decorative level={0} />);
    expect(container.querySelector(".amara-lens__halo")).toBeNull();
    expect(container.querySelector(".amara-lens__catchlight")).toBeNull();
    expect(container.querySelector(".amara-lens")).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("30 Hz levels drive CSS vars without re-rendering React", async () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "performance", "Date", "setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
    let renders = 0;
    const { container } = render(<Profiler id="lens" onRender={() => { renders++; }}><AmaraLens state="speaking" size={160} /></Profiler>);
    await flush();
    const before = renders;
    for (let i = 0; i < 150; i++) {
      emit(AMARA_VOICE_LEVEL_EVENT, { rms: i % 2 ? 1 : 0.2, peak: 1, state: "speaking" });
      act(() => { vi.advanceTimersByTime(33); });
    }
    expect(renders).toBe(before + 1); // the one state transition idle→speaking from the first event
    const el = container.querySelector<HTMLElement>(".amara-lens")!;
    const scale = Number(el.style.getPropertyValue("--lens-scale"));
    expect(scale).toBeGreaterThan(1);
    expect(scale).toBeLessThanOrEqual(1.12);
  });

  it("unlistens on unmount", async () => {
    const { unmount } = render(<AmaraLens state="idle" size={44} />);
    await flush();
    expect(events.handlers.size).toBe(2);
    unmount();
    expect(events.unlisten).toHaveBeenCalledWith(AMARA_VOICE_LEVEL_EVENT);
    expect(events.unlisten).toHaveBeenCalledWith(AMARA_VOICE_STATE_EVENT);
  });
});

describe("useAmaraVoice store", () => {
  it("sanitises (peak >= rms), drops stale and older-utterance events", async () => {
    render(<AmaraVoiceAnnouncer />);
    await flush();
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 7, peak: NaN, state: "speaking", utteranceId: "b" });
    expect(amaraVoiceLevel.current).toEqual({ rms: 1, peak: 1 });
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 0.3, peak: 0.3, ts: Date.now() - 600 });
    expect(amaraVoiceLevel.current.rms).toBe(1);
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 0.3, peak: 0.3, utteranceId: "a", ts: Date.now() - 100 });
    expect(amaraVoiceLevel.current.rms).toBe(1);
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 0.4, peak: 0.2 });
    expect(amaraVoiceLevel.current).toEqual({ rms: 0.4, peak: 0.4 });
    emit(AMARA_VOICE_STATE_EVENT, { state: "bogus" });
    expect(amaraVoiceLevel.current.rms).toBe(0.4);
  });

  it("2s without levels decays the meter only; state holds until ~12s without a state event", async () => {
    vi.useFakeTimers();
    render(<AmaraVoiceAnnouncer />);
    await flush();
    emit(AMARA_VOICE_STATE_EVENT, { state: "speaking" });
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 0.6, peak: 0.8 });
    act(() => { vi.advanceTimersByTime(LEVEL_SILENCE_MS + 300); });
    expect(amaraVoiceLevel.current).toEqual({ rms: 0, peak: 0 });
    expect(screen.getByTestId("amara-voice-announcer")).toHaveTextContent("Amara is speaking");
    // 5s heartbeat keeps the state alive.
    act(() => { vi.advanceTimersByTime(4000); });
    emit(AMARA_VOICE_STATE_EVENT, { state: "speaking" });
    act(() => { vi.advanceTimersByTime(STATE_FALLBACK_MS - 1000); });
    expect(screen.getByTestId("amara-voice-announcer")).toHaveTextContent("Amara is speaking");
    act(() => { vi.advanceTimersByTime(1600); });
    act(() => { vi.advanceTimersByTime(600); });
    expect(screen.getByTestId("amara-voice-announcer")).toHaveTextContent("Amara is idle");
  });

  it("error/unavailable shows the error copy; idle/model_loading reports loading", async () => {
    let snap: ReturnType<typeof useAmaraVoice> | null = null;
    function Probe() { snap = useAmaraVoice(); return null; }
    render(<><Probe /><AmaraLens state="idle" size={160} /></>);
    await flush();
    emit(AMARA_VOICE_STATE_EVENT, { state: "idle", reason: "model_loading" });
    expect(snap!.loading).toBe(true);
    emit(AMARA_VOICE_STATE_EVENT, { state: "error", reason: "unavailable" });
    expect(snap!.state).toBe("error");
    expect(snap!.reason).toBe("unavailable");
    expect(snap!.loading).toBe(false);
  });

  it("voice bridge is ref-counted across live lenses; fixed-level lenses and the announcer don't start it", async () => {
    const { rerender, unmount } = render(<><AmaraVoiceAnnouncer /><AmaraLens key="d" state="idle" size={44} level={0.5} decorative /></>);
    await flush();
    expect(core.invoke).not.toHaveBeenCalled();
    rerender(<><AmaraVoiceAnnouncer /><AmaraLens key="d" state="idle" size={44} level={0.5} decorative /><AmaraLens key="a" state="idle" size={160} /><AmaraLens key="b" state="idle" size={28} /></>);
    await flush();
    expect(core.invoke.mock.calls).toEqual([["voice_bridge_start"]]);
    rerender(<><AmaraVoiceAnnouncer /><AmaraLens key="a" state="idle" size={160} /></>);
    await flush();
    expect(core.invoke).toHaveBeenCalledTimes(1);
    unmount();
    expect(core.invoke.mock.calls).toEqual([["voice_bridge_start"], ["voice_bridge_stop"]]);
  });

  it("a missing bridge command is tolerated (stays idle, no error)", async () => {
    core.invoke.mockRejectedValueOnce("Command voice_bridge_start not found");
    let snap: ReturnType<typeof useAmaraVoice> | null = null;
    function Probe() { snap = useAmaraVoice(); return null; }
    render(<><Probe /><AmaraLens state="idle" size={160} /></>);
    await flush();
    expect(snap!.state).toBe("idle");
  });

  it("live region announces each transition once (debounced), never per level", async () => {
    vi.useFakeTimers();
    render(<AmaraVoiceAnnouncer />);
    await flush();
    const region = screen.getByTestId("amara-voice-announcer");
    emit(AMARA_VOICE_STATE_EVENT, { state: "listening" });
    emit(AMARA_VOICE_STATE_EVENT, { state: "thinking" });
    act(() => { vi.advanceTimersByTime(100); });
    expect(region).toHaveTextContent("");
    act(() => { vi.advanceTimersByTime(500); });
    expect(region).toHaveTextContent("Amara is thinking");
    act(() => ingestState({ ts: Date.now(), state: "error", reason: "tts_failed" }));
    act(() => { vi.advanceTimersByTime(600); });
    expect(region).toHaveTextContent(LENS_ERROR_COPY);
  });
});
