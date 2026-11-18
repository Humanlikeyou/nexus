import { useEffect, useRef, useState } from "react";
import { AmaraLens } from "../components/AmaraLens/AmaraLens";
import type { AmaraVoiceState } from "../hooks/useAmaraVoice";
import { amaraVoiceLevel, amaraVoiceLevelAge } from "../hooks/useAmaraVoice";
import { ambienceVars, targetAmbience } from "./ambience";
import { fallbackPeaks } from "./bessel";
import type { SandEngine, SandMoment, SandState } from "./engine";
import { webgl2Available } from "./gl";
import { calmActive, sessionSeed, useSandSettings } from "./settings";
import { rustle, soundOut, water, wood } from "./sound";
import "./sandLens.css";

export const SAND_MOMENT_EVENT = "nexus:sand-moment";
/** Fire a micro-moment from anywhere: arrive | complete | notify. */
export function sandMoment(kind: SandMoment) { window.dispatchEvent(new CustomEvent(SAND_MOMENT_EVENT, { detail: kind })); }

/** Lens voice state → sand state. Voice off (or out of reach) = the sand rests on the chat card. */
export function sandStateFor(voice: AmaraVoiceState, line: string): SandState {
  if (voice === "speaking") return "speak";
  if (voice === "listening" || voice === "thinking") return "listen";
  if (voice === "error" && !/reach/i.test(line)) return "listen";
  return "off";
}

/** Capture/dev only: pin a particle tier (0 = densest). Users get the auto-scaler. */
const devTier = () => { try { const v = window.localStorage.getItem("nexus.sandTier"); return v == null ? undefined : Number(v); } catch { return undefined; } };
const nowMinutes = () => { const d = new Date(); return d.getHours() * 60 + d.getMinutes(); };

declare global { interface Window { __sand?: { engine: SandEngine | null; setTod: (m: number | null) => void; setRecovery: (r: number | null | undefined) => void; setState: (s: SandState | null) => void; moment: (k: SandMoment) => void; voice: (loud: number, onset?: number) => void } } }

/**
 * design-1118b: the sand lens on Home. One shared canvas, pointer-events: none (never blocks a click).
 * The lens area itself draws nothing (no ring, bezel or plate): only an invisible hit area remains where
 * the lens was, so the navy long-press exit and any orb click keep working. Classic lens = Settings;
 * no WebGL2 = the old static lens.
 */
export function SandLensLayer({ voice, line, recovery, active = true }: { voice: AmaraVoiceState; line: string; recovery: number | null | undefined; active?: boolean }) {
  const s = useSandSettings();
  const [gl] = useState(() => typeof window !== "undefined" && webgl2Available());
  const sand = s.lens === "sand" && gl;
  const hitRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const engineRef = useRef<SandEngine | null>(null);
  const [failed, setFailed] = useState(false);
  const override = useRef<{ tod: number | null; rec: number | null | undefined; state: SandState | null }>({ tod: null, rec: undefined, state: null });
  const target = sandStateFor(voice, line);
  const prev = useRef<SandState>("off");
  const calm = calmActive(s);

  // engine lifetime: `three` + the engine load on demand (a separate chunk), so the app shell stays light
  const [ready, setReady] = useState(0);
  useEffect(() => {
    if (!sand || !active || !canvasRef.current) return;
    const canvas = canvasRef.current;
    let engine: SandEngine | null = null; let gone = false;
    import("./engine").then(({ SandEngine: Engine }) => {
      if (gone) return;
      engine = new Engine(canvas, () => {
        const r = hitRef.current?.getBoundingClientRect();
        const card = document.querySelector(".landing .hchat")?.getBoundingClientRect() ?? null;
        return r ? { cx: r.left + r.width / 2, cy: r.top + r.height / 2, R: r.width / 2, card } : { cx: window.innerWidth / 2, cy: window.innerHeight / 3, R: 130, card };
      }, sessionSeed(), { tier: devTier() });
      engineRef.current = engine;
      engine.start();
      setReady((n) => n + 1);
    }).catch((e) => { console.warn("[sand] falling back to the static lens", e); if (!gone) setFailed(true); });
    return () => { gone = true; engine?.dispose(); engineRef.current = null; };
  }, [sand, active]);

  // state (wake first when the voice comes on)
  useEffect(() => {
    const e = engineRef.current; if (!e) return;
    const want = override.current.state ?? target;
    if (prev.current === "off" && want !== "off") { e.setState("wake"); e.onWakeDone = () => { e.setState(want === "speak" ? "speak" : "listen"); }; }
    else e.setState(want);
    if (prev.current !== want && s.sound) { const g = soundOut(s.volume, s.mute); if (g) rustle(g); }
    prev.current = want;
  }, [target, s.sound, s.volume, s.mute, ready]);

  useEffect(() => { engineRef.current?.setMaterial(s.material); }, [s.material, sand, ready]);
  useEffect(() => { engineRef.current?.setCalm(calm); }, [calm, sand, ready]);

  // ambience (step c) + light tokens (step b): CSS variables on the landing root, eased by the engine loop.
  useEffect(() => {
    const root = document.documentElement;
    let raf = 0;
    const apply = () => {
      const rec = override.current.rec !== undefined ? override.current.rec : recovery ?? null;
      const a = targetAmbience(override.current.tod ?? nowMinutes(), s.ambience ? rec : null, calm);
      const vars = s.tokens || s.ambience ? ambienceVars(a, s.ambience) : {};
      for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
      root.setAttribute("data-sand-tokens", s.tokens ? "on" : "off");
      root.setAttribute("data-sand-ambience", s.ambience ? (a.source === "recovery" ? "recovery" : "neutral") : "off");
      engineRef.current?.setAmbience(s.ambience || s.tokens ? a : null, s.ambience);
    };
    apply();
    const id = window.setInterval(apply, 30_000);
    const onKick = () => { cancelAnimationFrame(raf); raf = requestAnimationFrame(apply); };
    window.addEventListener("nexus:sand-ambience", onKick);
    return () => { window.clearInterval(id); window.removeEventListener("nexus:sand-ambience", onKick); cancelAnimationFrame(raf); for (const k of ["--light-x", "--light-y", "--light-angle", "--shx", "--shy", "--edge", "--bg-a", "--bg-b", "--bg-c"]) root.style.removeProperty(k); root.removeAttribute("data-sand-tokens"); root.removeAttribute("data-sand-ambience"); };
  }, [s.tokens, s.ambience, calm, recovery, sand, ready]);

  // voice level → Chladni amplitude; loudness onsets advance the speech-band modes (no spectrum in the UI)
  useEffect(() => {
    if (!sand) return;
    let raf = 0, onsets = 0, wasQuiet = true, env = 0;
    const tick = () => {
      const e = engineRef.current;
      if (e) {
        const fresh = amaraVoiceLevelAge() < 2000;
        const rms = fresh ? amaraVoiceLevel.current.rms : 0;
        const loud = Math.min(1, Math.max(0, rms * 3.2));
        env += (loud - env) * 0.3;
        if (wasQuiet && env > 0.18) { onsets++; wasQuiet = false; } else if (env < 0.06) wasQuiet = true;
        if (target === "speak") e.setVoice(env, fallbackPeaks(onsets));
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [sand, target]);

  // sound on figure changes (off by default)
  useEffect(() => {
    const e = engineRef.current; if (!e) return;
    let last = 0;
    e.onModeChange = (m) => { if (!s.sound || calm) return; const now = performance.now(); if (now - last < 300) return; last = now; const g = soundOut(s.volume, s.mute); if (g) (Math.random() < 0.6 ? wood : water)(m.f, g); };
  }, [s.sound, s.volume, s.mute, calm, sand, ready]);

  // micro-moments (step e)
  useEffect(() => {
    if (!s.moments) return;
    const on = (ev: Event) => { const k = (ev as CustomEvent<SandMoment>).detail; engineRef.current?.moment(k); };
    window.addEventListener(SAND_MOMENT_EVENT, on);
    return () => window.removeEventListener(SAND_MOMENT_EVENT, on);
  }, [s.moments, sand]);
  useEffect(() => { if (sand && s.moments && ready === 1) { const t = window.setTimeout(() => engineRef.current?.moment("arrive"), 50); return () => window.clearTimeout(t); } }, [sand, s.moments, ready]);

  // capture / test hooks (no-ops for users)
  useEffect(() => {
    window.__sand = {
      get engine() { return engineRef.current; },
      setTod: (m) => { override.current.tod = m; window.dispatchEvent(new Event("nexus:sand-ambience")); },
      setRecovery: (r) => { override.current.rec = r; window.dispatchEvent(new Event("nexus:sand-ambience")); },
      setState: (st) => { override.current.state = st; const e = engineRef.current; if (e) { e.setState(st ?? target); prev.current = st ?? target; } },
      moment: (k) => engineRef.current?.moment(k),
      voice: (loud, onset = 0) => engineRef.current?.setVoice(loud, fallbackPeaks(onset)),
    };
    return () => { delete window.__sand; };
  }, [target]);

  if (!sand || failed) return <AmaraLens state={voice} size={320} decorative />;
  return (
    <>
      {/* invisible: keeps the navy long-press and any orb click working where the lens was */}
      <div ref={hitRef} className="sand-lens__hit" data-testid="lens-hit" data-sand-state={target} aria-hidden="true" />
      {active && <canvas ref={canvasRef} className="sand-lens__canvas" data-testid="sand-canvas" data-material={s.material} aria-hidden="true" style={{ pointerEvents: "none" }} />}
    </>
  );
}
