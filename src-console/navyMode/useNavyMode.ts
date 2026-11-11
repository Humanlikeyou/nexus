import { useEffect, useRef } from "react";
import { handleNavyPrompt } from "./store";

/** Intercept new spoken turns after mount. The first poll is historical and is skipped. */
export function useVoiceNavyPrompts(turns: { time?: string; heard?: string; text?: string }[]): void {
  const primed = useRef(false);
  const seen = useRef("");
  useEffect(() => {
    const last = turns.length ? turns[turns.length - 1] : undefined;
    const key = last ? `${last.time ?? ""}:${last.heard ?? last.text ?? ""}` : "";
    if (!primed.current) { primed.current = true; seen.current = key; return; }
    if (!last || seen.current === key) return;
    seen.current = key;
    const q = (last.heard || last.text || "").trim();
    if (q) handleNavyPrompt(q, "voice");
  }, [turns]);
}
