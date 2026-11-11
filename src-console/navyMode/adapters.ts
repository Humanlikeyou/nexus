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

export function navyAdapter(): NavyAdapter {
  return sampleAdaptersEnabled() ? sampleNavy : unwiredNavy;
}
