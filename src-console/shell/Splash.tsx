import { useEffect, useRef, useState } from "react";
import "./Splash.css";

type Phase = "wait" | "show" | "fade" | "done";

/** Boot splash (screen-specs › Shell › Wordmark › Splash). No artificial delay: it appears only if the
 *  first snapshot takes longer than 300 ms, fades out (200 ms) as soon as it lands, and never stays past 1.5 s. */
export function Splash({ loaded }: { loaded: boolean }) {
  const [phase, setPhase] = useState<Phase>("wait");
  const loadedRef = useRef(loaded);
  useEffect(() => { loadedRef.current = loaded; }, [loaded]);
  useEffect(() => {
    const show = window.setTimeout(() => setPhase((p) => (p !== "wait" ? p : loadedRef.current ? "done" : "show")), 300);
    const cap = window.setTimeout(() => setPhase((p) => (p === "show" ? "fade" : p === "wait" ? "done" : p)), 1500);
    return () => { window.clearTimeout(show); window.clearTimeout(cap); };
  }, []);
  const fading = phase === "fade" || (phase === "show" && loaded);
  useEffect(() => {
    if (!fading) return;
    const t = window.setTimeout(() => setPhase("done"), 200);
    return () => window.clearTimeout(t);
  }, [fading]);
  if (phase === "wait" || phase === "done") return null;
  return (
    <div className="splash" data-fading={fading || undefined} role="status" aria-label="Amara OS is loading">
      <span className="wordmark wordmark--splash" aria-hidden="true">Amara</span>
    </div>
  );
}
