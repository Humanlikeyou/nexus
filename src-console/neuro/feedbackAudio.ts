/**
 * Decision 0059 (step 8c): what the feedback sound does for each feedback state, as plain numbers.
 *
 * - `reward`: the clear cue (tone up, or the soundscape at its softest and darkest).
 * - `neutral` / `inhibit`: quieter or brighter, never the reward cue.
 * - `held`: feedback is paused (noise, a blink, a frozen or non-live signal). The tone is silent and the
 *   soundscape sits low and steady, so a pause can never sound like reward.
 * - `off`, or muted: everything at zero. Muting really silences what is already playing.
 *
 * Volume (0–1) scales every level. Changes ramp over RAMP_S seconds, never jump.
 */
import type { ProtocolSound } from "./protocolConfig";

export type FeedbackCue = "reward" | "neutral" | "inhibit" | "held" | "off";
export type AudioTargets = { ambient: number; tone: number; filterHz: number };

export const RAMP_S = 0.6;
const BASE: Record<ProtocolSound, Record<Exclude<FeedbackCue, "off">, AudioTargets>> = {
  soundscape: {
    reward: { ambient: 0.15, tone: 0, filterHz: 200 },
    neutral: { ambient: 0.25, tone: 0, filterHz: 500 },
    inhibit: { ambient: 0.4, tone: 0, filterHz: 1200 },
    held: { ambient: 0.06, tone: 0, filterHz: 300 },
  },
  tone: {
    reward: { ambient: 0.05, tone: 0.2, filterHz: 300 },
    neutral: { ambient: 0.05, tone: 0.02, filterHz: 300 },
    inhibit: { ambient: 0.05, tone: 0.02, filterHz: 300 },
    held: { ambient: 0.03, tone: 0, filterHz: 300 },
  },
  silent: {
    reward: { ambient: 0, tone: 0, filterHz: 300 },
    neutral: { ambient: 0, tone: 0, filterHz: 300 },
    inhibit: { ambient: 0, tone: 0, filterHz: 300 },
    held: { ambient: 0, tone: 0, filterHz: 300 },
  },
};

export function audioTargets(mode: ProtocolSound, cue: FeedbackCue, volume: number, muted: boolean): AudioTargets {
  const v = Number.isFinite(volume) ? Math.min(1, Math.max(0, volume)) : 0;
  if (muted || cue === "off" || v === 0) return { ambient: 0, tone: 0, filterHz: 300 };
  const t = BASE[mode][cue];
  return { ambient: t.ambient * v, tone: t.tone * v, filterHz: t.filterHz };
}

/** The cue for a training state: held whenever feedback is gated or paused. */
export function cueFor(state: { stage?: string; phase: string; signal: { action: string; gated: boolean } | null }): FeedbackCue {
  if (state.stage !== "training") return "off";
  if (state.phase === "held" || !state.signal || state.signal.gated) return "held";
  return state.signal.action === "reward" ? "reward" : state.signal.action === "inhibit" ? "inhibit" : "neutral";
}
