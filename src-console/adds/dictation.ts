import { invokeBound } from "../bridge";
import type { Invoke } from "../lib/localStores";
import { sampleAdaptersEnabled } from "../lib/sampleAdapters";

/**
 * One on-device dictation (voice.rs `dictate_once`): Amara Voice records once from its own mic and transcribes
 * with its local Whisper; only text comes back, never audio. Inert until the patched loop announces itself
 * (tasks/proposed/amara-voice-dictate.diff). No navy gate: it is always on-device. The text is for the
 * existing add / check-in preview; nothing here saves. Never called by the typed path.
 */
export type DictatePurpose = "add" | "checkin";
export const DICTATE_MAX_SECS = 20;
export const DICTATE_MAX_CHARS = 500;

const call: Invoke = (c, a) => invokeBound(c, a);

/** True only when the backend says a patched Amara Voice loop is running. Any error means not ready. */
export async function dictationReady(run: Invoke = call): Promise<boolean> {
  if (sampleAdaptersEnabled()) return false;
  try { return (await run<boolean>("voice_dictate_ready")) === true; } catch { return false; }
}

/** The transcript (trimmed, capped), or null when nothing was heard / not ready / any error. */
export async function dictateOnce(purpose: DictatePurpose, run: Invoke = call): Promise<string | null> {
  if (sampleAdaptersEnabled()) return null;
  try {
    const t = await run<string | null>("dictate_once", { purpose, maxSecs: DICTATE_MAX_SECS });
    const text = typeof t === "string" ? t.replace(/\s+/g, " ").trim().slice(0, DICTATE_MAX_CHARS) : "";
    return text || null;
  } catch { return null; }
}
