import { invokeBound } from "../bridge";
import { sample, sampleAdaptersEnabled, UNWIRED, type Wired } from "../lib/sampleAdapters";

/** Voice intent + local-only spoken line (light tier, cloud off). UI toggles the look itself. */
export type NavySpeak = { text: string };

export interface NavyAdapter {
  /** Speak a line locally. Unwired until on-device TTS lands. Sample mode returns the text, tagged Sample data. */
  speak(text: string): Wired<NavySpeak>;
  /** Confirm the line stays on this Mac (light tier). Unwired until the local-only path lands. */
  localOnly(): Wired<{ ok: true }>;
}

export const unwiredNavy: NavyAdapter = {
  speak: () => UNWIRED,
  localOnly: () => UNWIRED,
};

export const sampleNavy: NavyAdapter = {
  speak: (text) => sample({ text }),
  localOnly: () => sample({ ok: true as const }),
};

/** True only after the backend confirmed its session-only local-only switch is on (set_local_only). */
let localOnlyConfirmed = false;

/**
 * Turns the backend's "nothing leaves this Mac" switch on or off (src-tauri telegram.rs
 * set_local_only: blocks Telegram, outbox, research and the Claude tier, like
 * NEXUS_OUTBOUND_DISABLED; in memory only, off again on restart). Fails closed: if the call
 * fails, localOnly() stays unwired rather than claiming a guarantee it can't confirm.
 */
export async function setNavyLocalOnly(on: boolean): Promise<boolean> {
  if (sampleAdaptersEnabled()) return on;
  try {
    const now = await invokeBound<boolean>("set_local_only", { on });
    localOnlyConfirmed = on && now === true;
    return now === true;
  } catch {
    localOnlyConfirmed = false;
    return false;
  }
}

/**
 * Live adapter: localOnly() is live once the backend confirmed it.
 *
 * speak() is deliberately UNWIRED. The existing voice path (src-tauri voice.rs / voice_bridge.rs +
 * the amara-voice loop) has no "speak this text" entry point: it only speaks its own replies to
 * what it heard, and exposes status, voices and stop-speaking. Inventing one (or falling back to
 * any cloud TTS) is not allowed, so nothing here speaks. The store already only asks to speak when
 * openedBy === "voice"; when a local entry point exists, wire it here and fail closed on error.
 */
export const liveNavy: NavyAdapter = {
  speak: () => UNWIRED,
  localOnly: () => (localOnlyConfirmed ? { state: "live", value: { ok: true as const } } : UNWIRED),
};

export function navyAdapter(): NavyAdapter {
  return sampleAdaptersEnabled() ? sampleNavy : liveNavy;
}
