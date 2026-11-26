import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { audioTargets, cueFor, type FeedbackCue } from "./feedbackAudio";
import { useAudioFeedback } from "../legacy-core/hooks/useAudioFeedback";

const CUES: FeedbackCue[] = ["reward", "neutral", "inhibit", "held"];

describe("decision 0059: feedback sound levels", () => {
  it("held never sounds like reward, in any mode", () => {
    for (const mode of ["soundscape", "tone"] as const) {
      const held = audioTargets(mode, "held", 1, false);
      const reward = audioTargets(mode, "reward", 1, false);
      expect(held.tone).toBe(0);
      expect(held).not.toEqual(reward);
      // Quieter than every live cue, so a pause is heard as a pause.
      for (const cue of ["reward", "neutral", "inhibit"] as const) expect(held.ambient).toBeLessThan(audioTargets(mode, cue, 1, false).ambient + 1e-9);
    }
  });

  it("the tone rises only for reward", () => {
    const tone = (cue: FeedbackCue) => audioTargets("tone", cue, 1, false).tone;
    expect(tone("reward")).toBeGreaterThan(tone("neutral"));
    expect(tone("inhibit")).toBeLessThan(tone("reward"));
  });

  it("muted, off, silent and zero volume are all silence", () => {
    for (const cue of CUES) {
      expect(audioTargets("tone", cue, 1, true)).toMatchObject({ ambient: 0, tone: 0 });
      expect(audioTargets("silent", cue, 1, false)).toMatchObject({ ambient: 0, tone: 0 });
      expect(audioTargets("soundscape", cue, 0, false)).toMatchObject({ ambient: 0, tone: 0 });
    }
    expect(audioTargets("tone", "off", 1, false)).toMatchObject({ ambient: 0, tone: 0 });
  });

  it("volume scales every level and is clamped", () => {
    const full = audioTargets("tone", "reward", 1, false);
    const half = audioTargets("tone", "reward", 0.5, false);
    expect(half.tone).toBeCloseTo(full.tone / 2);
    expect(audioTargets("tone", "reward", 7, false)).toEqual(full);
    expect(audioTargets("tone", "reward", Number.NaN, false).tone).toBe(0);
  });

  it("a gated or held state is the held cue; outside training is off", () => {
    expect(cueFor({ stage: "training", phase: "training", signal: { action: "reward", gated: false } })).toBe("reward");
    expect(cueFor({ stage: "training", phase: "training", signal: { action: "reward", gated: true } })).toBe("held");
    expect(cueFor({ stage: "training", phase: "held", signal: null })).toBe("held");
    expect(cueFor({ stage: "baseline-open", phase: "calibrating", signal: null })).toBe("off");
    expect(cueFor({ phase: "training", signal: { action: "reward", gated: false } })).toBe("off");
  });
});

/** A small fake Web Audio graph: enough to see where each gain ends up. */
class FakeParam {
  value: number;
  target: number;
  constructor(v: number) { this.value = v; this.target = v; }
  cancelScheduledValues() { /* nothing scheduled in the fake */ }
  setValueAtTime(v: number) { this.value = v; }
  linearRampToValueAtTime(v: number) { this.target = v; }
  exponentialRampToValueAtTime(v: number) { this.target = v; }
}
const gains: FakeParam[] = [];
class FakeNode { connect() { return this; } start() {} stop() {} }
class FakeAudioContext {
  currentTime = 0; sampleRate = 8000; destination = {};
  createGain() { const g = Object.assign(new FakeNode(), { gain: new FakeParam(0) }); gains.push(g.gain); return g; }
  createBuffer() { return { getChannelData: () => new Float32Array(16000) }; }
  createBufferSource() { return Object.assign(new FakeNode(), { buffer: null, loop: false }); }
  createBiquadFilter() { return Object.assign(new FakeNode(), { type: "", frequency: new FakeParam(500), Q: new FakeParam(0) }); }
  createOscillator() { return Object.assign(new FakeNode(), { type: "", frequency: new FakeParam(0) }); }
  close() { return Promise.resolve(); }
}

describe("decision 0059: mute really silences", () => {
  afterEach(() => { vi.unstubAllGlobals(); gains.length = 0; });

  it("muting ramps the playing tone and soundscape to zero, and unmuting restores the cue", () => {
    vi.stubGlobal("AudioContext", FakeAudioContext);
    const { result } = renderHook(() => useAudioFeedback());
    act(() => { result.current.init(); result.current.setMode("tone"); });
    // init() creates: ambient, drone, reward, tone gains (in that order).
    const [ambient, , , tone] = gains;
    act(() => result.current.applyCue("reward", 1));
    expect(tone.target).toBeCloseTo(0.2);
    act(() => result.current.setMuted(true));
    expect(tone.target).toBe(0);
    expect(ambient.target).toBe(0);
    act(() => result.current.setMuted(false));
    expect(tone.target).toBeCloseTo(0.2);
    act(() => result.current.applyCue("held", 1));
    expect(tone.target).toBe(0);
  });
});
