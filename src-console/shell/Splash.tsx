import { useEffect, useRef, useState } from "react";
import { Wordmark } from "../ui/Wordmark";
import "./Splash.css";

type Phase = "wait" | "show" | "fade" | "done";

/** Either reduced-motion switch (OS setting or Settings › Appearance › Reduce motion). */
export function reducedMotion(): boolean {
  if (typeof document !== "undefined" && document.documentElement.dataset.reduceMotion === "true") return true;
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Boot splash (screen-specs › Shell › Wordmark › Splash). No artificial delay: it appears only if the
 *  first snapshot takes longer than 300 ms, fades out (200 ms) as soon as it lands, and never stays past 1.5 s.
 *  Under reduced motion it skips the fade and is gone in the same frame (Decision 19). */
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
  // Reduced motion: no fade phase to watch, so it is gone in the same frame.
  if (phase === "wait" || phase === "done" || (fading && reducedMotion())) return null;
  return (
    <div className="splash" data-fading={fading || undefined} role="status" aria-label="Amara OS is loading">
      <div className="splash__lockup">
        <Wordmark size="splash" decorative />
        {/* Decision 13: subtitle under the mark; decorative, the status keeps its aria-label. */}
        <p className="splash__subtitle" aria-hidden="true">Command Center</p>
      </div>
    </div>
  );
}
