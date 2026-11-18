import { useEffect, useRef, useState } from "react";
import { AmaraLens } from "../components/AmaraLens/AmaraLens";
import type { AmaraVoiceState } from "../hooks/useAmaraVoice";
import { amaraVoiceLevel, amaraVoiceLevelAge } from "../hooks/useAmaraVoice";
import { useLiveHeartRate } from "../hooks/useLiveHeartRate";
import { pulsePeriod } from "./pulse";
import { buildModes, fallbackPeaks } from "./bessel";
import type { SandEngine, SandState } from "./engine";
import { webgl2Available } from "./gl";
import { calmActive, sessionSeed, useSandSettings } from "./settings";
import { rustle, soundOut, water, wood } from "./sound";
import "./sandLens.css";

/** Lens voice state → sand state. Voice off (or out of reach) = a calm floating layer, gently breathing. */
export function sandStateFor(voice: AmaraVoiceState, line: string): SandState {
  if (voice === "speaking") return "speak";
  if (voice === "listening" || voice === "thinking") return "listen";
  if (voice === "error" && !/reach/i.test(line)) return "listen";
  return "off";
}

/** Capture/dev only: pin a particle tier (0 = densest). Users get the auto-scaler. */
const devTier = () => { try { const v = window.localStorage.getItem("nexus.sandTier"); return v == null ? undefined : Number(v); } catch { return undefined; } };

declare global { interface Window { __sand?: { engine: SandEngine | null; setState: (s: SandState | null) => void; voice: (loud: number, onset?: number) => void; tone: (mode: string, loud?: number) => void } } }

/**
 * design-1118b: the sand lens on Home. One shared canvas, pointer-events: none (never blocks a click).
 * The lens area itself draws nothing (no ring, bezel or plate): only an invisible hit area remains where
 * the lens was, so the navy long-press exit and any orb click keep working. Classic lens = Settings;
 * no WebGL2 = the old static lens.
 */
export function SandLensLayer({ voice, line, active = true }: { voice: AmaraVoiceState; line: string; active?: boolean }) {
  const s = useSandSettings();
  const [gl] = useState(() => typeof window !== "undefined" && webgl2Available());
  const sand = s.lens === "sand" && gl;
  const hitRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const engineRef = useRef<SandEngine | null>(null);
  const [failed, setFailed] = useState(false);
  const override = useRef<{ state: SandState | null }>({ state: null });
  const target = sandStateFor(voice, line);
  const prev = useRef<SandState>("off");
  const calm = calmActive(s);
  // heartbeat: the same live heart-rate feed as the Home heart card; no data or a stale reading = no pulse
  const heart = useLiveHeartRate();
  const period = pulsePeriod(heart?.bpm, heart?.at);

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
        return r ? { cx: r.left + r.width / 2, cy: r.top + r.height / 2, R: r.width / 2 } : { cx: window.innerWidth / 2, cy: window.innerHeight / 3, R: 130 };
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
  useEffect(() => { engineRef.current?.setPulse(period); }, [period, sand, ready]);

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

  // capture / test hooks (no-ops for users)
  useEffect(() => {
    window.__sand = {
      get engine() { return engineRef.current; },
      setState: (st) => { override.current.state = st; const e = engineRef.current; if (e) { e.setState(st ?? target); prev.current = st ?? target; } },
      voice: (loud, onset = 0) => engineRef.current?.setVoice(loud, fallbackPeaks(onset)),
      // pure test tone at one plate mode's frequency (e.g. "n2m2")
      tone: (mode, loud = 0.85) => { const m = buildModes().modes.find((x) => x.id === mode); if (m) engineRef.current?.setVoice(loud, [{ f: m.f, amp: 1 }]); },
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
