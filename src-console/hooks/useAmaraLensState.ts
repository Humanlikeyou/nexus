import { useEffect, useState } from "react";
import { amaraVoiceLevel, amaraVoiceLevelAge, useAmaraVoice, LEVEL_SILENCE_MS, type AmaraVoiceState } from "./useAmaraVoice";

/**
 * design-1118b backend: Amara's lens state and voice level for the Home sand lens.
 *
 * There is no Web Audio AnalyserNode on Amara's speech in this app: speech is synthesised and played by the
 * separate amara-voice process, so the webview never holds her audio. The Rust voice bridge relays that
 * process's loudness (ws 127.0.0.1:8791/level) as `amara-voice://level` (rms/peak, up to 30 Hz), and
 * `useAmaraVoice` keeps the latest frame in `amaraVoiceLevel`. `voiceLoudness()` below is the AnalyserNode
 * stand-in: a 0..1 value a render loop reads each frame. The spectrum comes separately: while she speaks,
 * amara-voice measures the 1-3 strongest peaks of what it plays and the frame carries them
 * (`amaraVoiceLevel.current.peaks`, decision 0040); the lens uses them, and falls back to loudness onsets
 * (sandLens/bessel.ts `fallbackPeaks`) only when a frame has none.
 * No audio, transcript or health value passes through here.
 */
export type AmaraLensState = "off" | "wake" | "listening" | "speaking";
/** How long the wake beat lasts after the voice comes on (matches the sand engine's ~2.2 s wake). */
export const WAKE_MS = 2200;
/** The lens' loudness gain: speech rms is small, so scale before clamping (same constant the lens uses). */
export const LOUDNESS_GAIN = 3.2;

/** Settled state from the voice state. `thinking` counts as listening; an `error` that is not "out of reach" too. */
export function settledLensState(voice: AmaraVoiceState, line = ""): Exclude<AmaraLensState, "wake"> {
  if (voice === "speaking") return "speaking";
  if (voice === "listening" || voice === "thinking") return "listening";
  if (voice === "error" && !/reach/i.test(line)) return "listening";
  return "off";
}

/** 0..1 loudness of Amara's speech right now. A silent or dead feed (no frame for 2 s) is 0, never the last value. */
export function voiceLoudness(): number {
  if (amaraVoiceLevelAge() >= LEVEL_SILENCE_MS) return 0;
  const rms = amaraVoiceLevel.current.rms;
  return Math.min(1, Math.max(0, (Number.isFinite(rms) ? rms : 0) * LOUDNESS_GAIN));
}

/** The typed Amara state: off, wake (for WAKE_MS after leaving off), listening, speaking. */
export function useAmaraLensState(line = ""): { state: AmaraLensState; loudness: () => number } {
  const { state: voice } = useAmaraVoice();
  const settled = settledLensState(voice, line);
  // wake = the first WAKE_MS after leaving off. Adjusted during render (no setState in an effect): `woke` is the
  // settled state we last saw, `waking` flips on the off -> on edge and off again from the timer below.
  const [woke, setWoke] = useState<Exclude<AmaraLensState, "wake">>("off");
  const [waking, setWaking] = useState(false);
  if (woke !== settled) {
    setWoke(settled);
    setWaking(woke === "off" && settled !== "off");
  }
  useEffect(() => {
    if (!waking) return undefined;
    const t = window.setTimeout(() => setWaking(false), WAKE_MS);
    return () => window.clearTimeout(t);
  }, [waking]);
  return { state: waking && settled !== "off" ? "wake" : settled, loudness: voiceLoudness };
}
