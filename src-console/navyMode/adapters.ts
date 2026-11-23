import { invokeBound } from "../bridge";
import { sample, sampleAdaptersEnabled, UNWIRED, type Wired } from "../lib/sampleAdapters";
import type { NavySource } from "./types";

/** Voice intent + local-only spoken line (light tier, cloud off). UI toggles the look itself. */
export type NavySpeak = { text: string };

export interface NavyAdapter {
  /**
   * Speak a line locally (Amara Voice's own on-device voice). Live only when `openedBy` is "voice" and the
   * local speak entry was confirmed; otherwise unwired. Sample mode returns the text, tagged Sample data.
   */
  speak(text: string, opts?: { openedBy?: NavySource }): Wired<NavySpeak>;
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

/** Same cap as the Rust side (voice.rs SPEAK_MAX_CHARS); the backend sanitises again. */
export const NAVY_SPEAK_MAX_CHARS = 240;

export function sanitizeSpeakText(raw: string): string {
  // eslint-disable-next-line no-control-regex
  const flat = raw.replace(/[\u0000-\u001f\u007f-\u009f]/g, " ").replace(/\s+/g, " ").trim();
  return Array.from(flat).slice(0, NAVY_SPEAK_MAX_CHARS).join("").trim();
}

/** True only after the backend confirmed a patched Amara Voice loop announced its local speak entry. */
let speakReady = false;
const speakFailed = new Set<() => void>();

/** Called when a line the adapter said it was speaking could not be handed to Amara Voice. */
export function onNavySpeakFailed(cb: () => void): () => void {
  speakFailed.add(cb);
  return () => { speakFailed.delete(cb); };
}

/**
 * Asks the backend whether Amara Voice can speak a handed-over line locally (voice.rs voice_speak_ready:
 * loop running + speak entry announced by the patched loop). Fails closed: any error, or an
 * unpatched / stopped Amara Voice, leaves speak() unwired. Only ever called for voice-opened looks.
 */
export async function confirmNavySpeak(): Promise<boolean> {
  if (sampleAdaptersEnabled()) return false;
  try {
    speakReady = (await invokeBound<boolean>("voice_speak_ready")) === true;
  } catch {
    speakReady = false;
  }
  return speakReady;
}

/**
 * Live adapter: localOnly() is live once the backend confirmed it.
 *
 * speak() hands one capped, sanitised line to Amara Voice through a local file request
 * (voice_speak_local; no network, no cloud TTS, not macOS say). It speaks only when the look was opened
 * by voice, local-only is confirmed and the speak entry was confirmed; any error or refusal flips it back to
 * unwired and tells the store so the caption stops claiming it was spoken. The Amara Voice side is
 * tasks/proposed/amara-voice-speak.diff; until Taisun applies it, speak() stays unwired.
 */
export const liveNavy: NavyAdapter = {
  speak: (text, opts) => {
    if (opts?.openedBy !== "voice" || !localOnlyConfirmed || !speakReady) return UNWIRED;
    const line = sanitizeSpeakText(text);
    if (!line) return UNWIRED;
    const fail = () => { speakReady = false; speakFailed.forEach((cb) => cb()); };
    invokeBound<boolean>("voice_speak_local", { text: line }).then((ok) => { if (ok !== true) fail(); }, fail);
    return { state: "live", value: { text: line } };
  },
  localOnly: () => (localOnlyConfirmed ? { state: "live", value: { ok: true as const } } : UNWIRED),
};

export function navyAdapter(): NavyAdapter {
  return sampleAdaptersEnabled() ? sampleNavy : liveNavy;
}
