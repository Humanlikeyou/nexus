import { useEffect, useRef } from "react";
import { createTurnTracker, spokenText } from "../hooks/freshVoiceTurns";
import type { VoiceTurn } from "../types";
import { handleNavyPrompt } from "./store";

/**
 * Intercept new spoken turns. History that loads after an empty first poll is never replayed: only turns under
 * 2 minutes old that amara-voice acted on count (see freshVoiceTurns).
 */
export function useVoiceNavyPrompts(turns: VoiceTurn[]): void {
  const track = useRef(createTurnTracker());
  useEffect(() => {
    for (const turn of track.current(turns)) {
      const q = spokenText(turn);
      if (q) handleNavyPrompt(q, "voice");
    }
  }, [turns]);
}
