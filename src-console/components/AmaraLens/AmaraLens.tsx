import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { type AmaraVoiceState, amaraVoiceLevelAge, useAmaraVoice, useAmaraVoiceBridge } from "../../hooks/useAmaraVoice";
import { Button } from "../../ui";
import "./AmaraLens.css";

/** 320 is the landing page hero: 320px at most, CSS shrinks it to fit its container (AmaraLens.css). */
export type AmaraLensSize = 28 | 44 | 160 | 320;

export const LENS_LABEL: Record<AmaraVoiceState, string> = {
  idle: "Amara is idle",
  listening: "Amara is listening",
  thinking: "Amara is thinking",
  speaking: "Amara is speaking",
  error: "Amara can't speak right now",
};
export const LENS_ERROR_COPY = "Amara can't speak right now. Check Amara Voice.";

/**
 * Envelope follower: fast attack, soft release, so the light lands on each syllable and falls between
 * them. Tuned on her reference voice as the feed delivers it (Nov 5): 1024-sample blocks at 24 kHz,
 * about 23 new values a second.
 */
export const ATTACK_MS = 30;
export const RELEASE_MS = 120;
/** Reduced motion: the same signal, slowed into a gentle brightness change. */
export const CALM_ATTACK_MS = 180;
export const CALM_RELEASE_MS = 420;
export const MAX_SCALE_GAIN = 0.12;
export function stepEnvelope(env: number, target: number, dtMs: number, attackMs = ATTACK_MS, releaseMs = RELEASE_MS): number {
  const t = Math.min(1, Math.max(0, Number.isFinite(target) ? target : 0));
  const tau = t > env ? attackMs : releaseMs;
  const a = 1 - Math.exp(-Math.max(0, dtMs) / tau);
  return env + (t - env) * a;
}

/**
 * The feed's rms is linear and taken after her playback gain (AMARA_VOLUME, default 0.25), so normal
 * speech arrives at about 0.016 (median) to 0.05 (loudest). Used raw it barely moves the lens. Speech
 * is read in decibels: -44 dB is the edge of a word, -26 dB is her loudest, and the curve keeps the
 * top of the range for stressed syllables. Median speech lands near .4 and peaks near .9.
 */
export const LEVEL_FLOOR_DB = -44;
export const LEVEL_CEILING_DB = -26;
export const LEVEL_CURVE = 1.4;
/** No level frame for this long means no audio data, whatever the state says. */
export const LEVEL_FRESH_MS = 300;
export function levelToVisual(rms: number): number {
  if (!Number.isFinite(rms) || rms <= 0) return 0;
  const position = (20 * Math.log10(rms) - LEVEL_FLOOR_DB) / (LEVEL_CEILING_DB - LEVEL_FLOOR_DB);
  return Math.min(1, Math.max(0, position)) ** LEVEL_CURVE;
}
export const lensScale = (env: number) => 1 + MAX_SCALE_GAIN * Math.min(1, Math.max(0, env));

/** Either switch: the OS setting or Settings › Appearance › Reduce motion. */
const reducedMotion = () => document.documentElement.dataset.reduceMotion === "true"
  || (typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches);

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
    // Unchanged values must not touch the style.
    let written = "";
    const calm = reducedMotion();
    const write = (env: number) => {
      const value = env.toFixed(3);
      if (value === written) return;
      written = value;
      el.style.setProperty("--lens-level", value);
      el.style.setProperty("--lens-scale", calm ? "1" : lensScale(env).toFixed(4));
    };
    // Breath and thinking are compositor-only CSS, so only speaking costs a frame loop.
    const driven = state === "speaking";
    // A fixed level outside speaking (e.g. the inline Home reply) is static: no loop.
    if (!driven && levelProp.current !== undefined) { write(calm ? 0 : Math.min(1, Math.max(0, levelProp.current))); return; }
    let env = Number(el.style.getPropertyValue("--lens-level")) || 0;
    let last = performance.now();
    let frame = 0;
    /** Real audio only: a stale or missing feed reads as silence, even while the state says speaking. */
    const target = () => {
      if (!driven) return 0;
      if (levelProp.current !== undefined) return levelProp.current;
      return amaraVoiceLevelAge() < LEVEL_FRESH_MS ? levelToVisual(levelRef.current.rms) : 0;
    };
    const tick = (now: number) => {
      frame = 0;
      if (document.hidden) { env = 0; write(0); return; }
      env = stepEnvelope(env, target(), now - last, calm ? CALM_ATTACK_MS : ATTACK_MS, calm ? CALM_RELEASE_MS : RELEASE_MS);
      last = now;
      write(env);
      // Keep running while driven; otherwise run out the release curve and stop.
      if (driven || env > 0.002) frame = requestAnimationFrame(tick);
      else write(0);
    };
    const start = () => { if (!frame && (driven || env > 0.002)) { last = performance.now(); frame = requestAnimationFrame(tick); } };
    const onVisibility = () => {
      el.toggleAttribute("data-paused", document.hidden);
      if (!document.hidden) start();
    };
    document.addEventListener("visibilitychange", onVisibility);
    onVisibility();
    return () => { document.removeEventListener("visibilitychange", onVisibility); cancelAnimationFrame(frame); };
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
        {size >= 160 && <div className="amara-lens__hot" />}
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

/**
 * The one shared live region: announces state transitions only (debounced 500ms), never levels.
 * `message` replaces the copy derived from the feed, for a screen that resolves its own state.
 */
export function AmaraVoiceAnnouncer({ message }: { message?: string }) {
  const { state } = useAmaraVoice();
  const [text, setText] = useState("");
  const first = useRef(true);
  useEffect(() => {
    if (first.current) { first.current = false; return; }
    const id = window.setTimeout(() => setText(message ?? (state === "error" ? LENS_ERROR_COPY : LENS_LABEL[state])), ANNOUNCE_DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [state, message]);
  return <div className="ui-visually-hidden" role="status" aria-live="polite" data-testid="amara-voice-announcer">{text}</div>;
}
