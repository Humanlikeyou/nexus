import { Profiler } from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const events = vi.hoisted(() => ({
  handlers: new Map<string, (e: { payload: unknown }) => void>(),
  unlisten: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, h: (e: { payload: unknown }) => void) => {
    events.handlers.set(name, h);
    return () => { events.unlisten(name); events.handlers.delete(name); };
  }),
}));

import { AMARA_VOICE_LEVEL_EVENT, AMARA_VOICE_STATE_EVENT, WATCHDOG_MS, __resetAmaraVoiceForTests, amaraVoiceLevel, ingestState } from "../../hooks/useAmaraVoice";
import { AmaraLens, AmaraVoiceAnnouncer, LENS_ERROR_COPY, lensScale, stepEnvelope } from "./AmaraLens";

const emit = (name: string, payload: Record<string, unknown>) => act(() => { events.handlers.get(name)?.({ payload: { ts: Date.now(), ...payload } }); });
const flush = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });

beforeEach(() => { events.handlers.clear(); events.unlisten.mockClear(); });
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
  it("sanitises, drops stale and older-utterance events, and the watchdog returns to idle", async () => {
    vi.useFakeTimers();
    render(<AmaraVoiceAnnouncer />);
    await flush();
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 7, peak: NaN, state: "speaking", utteranceId: "b" });
    expect(amaraVoiceLevel.current).toEqual({ rms: 1, peak: 0 });
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 0.3, peak: 0.3, ts: Date.now() - 600 });
    expect(amaraVoiceLevel.current.rms).toBe(1);
    emit(AMARA_VOICE_LEVEL_EVENT, { rms: 0.3, peak: 0.3, utteranceId: "a", ts: Date.now() - 100 });
    expect(amaraVoiceLevel.current.rms).toBe(1);
    emit(AMARA_VOICE_STATE_EVENT, { state: "bogus" });
    act(() => { vi.advanceTimersByTime(WATCHDOG_MS + 300); });
    act(() => { vi.advanceTimersByTime(600); });
    expect(screen.getByTestId("amara-voice-announcer")).toHaveTextContent("Amara is idle");
    expect(amaraVoiceLevel.current.rms).toBe(0);
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
