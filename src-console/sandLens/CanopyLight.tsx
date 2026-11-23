import { useEffect, useRef, useState } from "react";
import { canopyFallbackStyle, canopySun, minutesNow, startCanopy } from "./canopy";
import { webgl2Available } from "./gl";
import { calmActive, sandSettings, useSandSettings } from "./settings";
import "./sandLens.css";

/**
 * design-1118b: a very gentle canopy-sunlight layer on Home. Soft dappled patches follow the time
 * of day. Calm / reduced motion freezes the drift (static light). No mist, parallax, or glass.
 * Works with the sand lens and the classic lens; the sand engine is left untouched.
 */
export function CanopyLight({ active = true }: { active?: boolean }) {
  const s = useSandSettings();
  const calm = calmActive(s);
  const on = !!s.tokens && active;
  const [gl] = useState(() => typeof window !== "undefined" && webgl2Available());
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!on || !gl || !canvasRef.current) return;
    const handle = startCanopy(canvasRef.current, { calm: () => calmActive(sandSettings()) });
    return () => handle.stop();
  }, [on, gl]);

  if (!on) return null;
  if (!gl) {
    const style = canopyFallbackStyle(canopySun(minutesNow()));
    return <div className="canopy-light canopy-light--css" data-testid="canopy-light" data-calm={calm || undefined} aria-hidden="true" style={style} />;
  }
  return <canvas ref={canvasRef} className="canopy-light" data-testid="canopy-light" data-calm={calm || undefined} aria-hidden="true" />;
}
