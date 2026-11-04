import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { type AmaraVoiceState, useAmaraVoice, useAmaraVoiceBridge } from "../../hooks/useAmaraVoice";
import { Button } from "../../ui";
import "./AmaraLens.css";

export type AmaraLensSize = 28 | 44 | 160;

export const LENS_LABEL: Record<AmaraVoiceState, string> = {
  idle: "Amara is idle",
  listening: "Amara is listening",
  thinking: "Amara is thinking",
  speaking: "Amara is speaking",
  error: "Amara can't speak right now",
};
export const LENS_ERROR_COPY = "Amara can't speak right now. Check Amara Voice.";

/** Envelope follower (spec §2): attack τ 40ms, release τ 160ms. */
export const ATTACK_MS = 40;
export const RELEASE_MS = 160;
export const MAX_SCALE_GAIN = 0.12;
export function stepEnvelope(env: number, target: number, dtMs: number): number {
  const t = Math.min(1, Math.max(0, Number.isFinite(target) ? target : 0));
  const tau = t > env ? ATTACK_MS : RELEASE_MS;
  const a = 1 - Math.exp(-Math.max(0, dtMs) / tau);
  return env + (t - env) * a;
}
export const lensScale = (env: number) => 1 + MAX_SCALE_GAIN * Math.min(1, Math.max(0, env));

const reducedMotion = () => typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export interface AmaraLensProps {
  state: AmaraVoiceState;
  size: AmaraLensSize;
  /** 0..1. When absent, the lens follows amara-voice://level (useAmaraVoice). */
  level?: number;
  /** Visible state label below (Voice panel). */
  showLabel?: boolean;
  /** Hide from assistive tech when adjacent text already says the same. */
  decorative?: boolean;
  /** Error state: optional Retry next to the message. */
  onRetry?: () => void;
  className?: string;
}

/**
 * Amara's speaking lens. Layers are CSS radial gradients; only transform and opacity animate.
 * Level drives CSS variables from requestAnimationFrame, so voice events never re-render React.
 */
export function AmaraLens({ state, size, level, showLabel = false, decorative = false, onRetry, className }: AmaraLensProps) {
  const { levelRef } = useAmaraVoice();
  // Only a live lens (no fixed level) needs the bridge running.
  useAmaraVoiceBridge(level === undefined);
  const root = useRef<HTMLDivElement>(null);
  const levelProp = useRef(level);
  useEffect(() => { levelProp.current = level; }, [level]);

  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const write = (env: number) => {
      el.style.setProperty("--lens-level", env.toFixed(3));
      el.style.setProperty("--lens-scale", lensScale(env).toFixed(4));
    };
    if (reducedMotion()) { write(0); return; }
    const driven = state === "speaking" || state === "listening";
    // A fixed level outside speaking/listening (e.g. the inline Home reply) is static: no loop.
    if (!driven && levelProp.current !== undefined) { write(Math.min(1, Math.max(0, levelProp.current))); return; }
    let env = Number(el.style.getPropertyValue("--lens-level")) || 0;
    let last = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const target = driven ? (levelProp.current ?? levelRef.current.rms) : 0;
      env = stepEnvelope(env, target, now - last);
      last = now;
      write(env);
      // Keep running while driven; otherwise run out the release curve and stop.
      if (driven || env > 0.002) frame = requestAnimationFrame(tick);
      else write(0);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [state, levelRef]);

  const label = LENS_LABEL[state];
  return (
    <span className={["amara-lens-wrap", className].filter(Boolean).join(" ")} data-size={size}>
      <div ref={root} className="amara-lens" data-state={state} data-size={size} data-lens-palette="amara"
        role={decorative ? undefined : "img"} aria-label={decorative ? undefined : label} aria-hidden={decorative || undefined}>
        {size !== 28 && <div className="amara-lens__halo" />}
        <div className="amara-lens__ring" />
        <div className="amara-lens__bezel" />
        <div className="amara-lens__glass" />
        <div className="amara-lens__core" />
        <div className="amara-lens__rim" />
        <div className="amara-lens__specular" />
        {size >= 32 && <div className="amara-lens__catchlight" />}
      </div>
      {showLabel && state !== "error" && <span className="amara-lens__label" aria-hidden="true">{label}</span>}
      {showLabel && state === "error" && (
        <span className="amara-lens__error">
          <span>{LENS_ERROR_COPY}</span>
          {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
        </span>
      )}
    </span>
  );
}

export const ANNOUNCE_DEBOUNCE_MS = 500;

/** The one shared live region: announces state transitions only (debounced 500ms), never levels. */
export function AmaraVoiceAnnouncer() {
  const { state } = useAmaraVoice();
  const [text, setText] = useState("");
  const first = useRef(true);
  useEffect(() => {
    if (first.current) { first.current = false; return; }
    const id = window.setTimeout(() => setText(state === "error" ? LENS_ERROR_COPY : LENS_LABEL[state]), ANNOUNCE_DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [state]);
  return <div className="ui-visually-hidden" role="status" aria-live="polite" data-testid="amara-voice-announcer">{text}</div>;
}
