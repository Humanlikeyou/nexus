import { useEffect, useState } from "react";
import { getVoiceTurns } from "../bridge";
import type { VoiceTurn } from "../types";

export const HOME_VOICE_TURNS = 20;
export const HOME_VOICE_TURNS_POLL_MS = 5_000;

/**
 * Recent spoken turns for the Home chat (design-1110b A2), polled while Home is showing and the
 * window is visible. Failures keep the last list: spoken turns are optional, typed chat still works.
 */
export function useVoiceTurns(enabled: boolean): VoiceTurn[] {
  const [turns, setTurns] = useState<VoiceTurn[]>([]);
  useEffect(() => {
    if (!enabled) return;
    let current = true;
    const load = () => {
      if (document.visibilityState === "hidden") return;
      getVoiceTurns(HOME_VOICE_TURNS).then((t) => { if (current && Array.isArray(t)) setTurns(t); }, () => { /* optional */ });
    };
    load();
    const timer = window.setInterval(load, HOME_VOICE_TURNS_POLL_MS);
    document.addEventListener("visibilitychange", load);
    return () => { current = false; window.clearInterval(timer); document.removeEventListener("visibilitychange", load); };
  }, [enabled]);
  return turns;
}
