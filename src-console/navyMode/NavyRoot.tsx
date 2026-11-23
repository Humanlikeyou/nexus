import { useEffect, useRef, useState, type ReactNode } from "react";
import { DataTag } from "../ui";
import {
  applyNavyPreview, beginHold, cancelHold, confirmHold, ENTER_MS, EXIT_MS, finishEnter, finishExit,
  HOLD_MS, navyStore, reachHoldConfirm, readNavyPreview, REDUCE_MS, reducedNavyMotion, setHoldAt, useNavyState,
} from "./store";
import "./navy.css";

const PREVIEW_BEAT: Record<string, number> = { tear: 1, flash: 2, cut: 3 };

function pinOrb(): void {
  const el = document.querySelector(".landing__lens .amara-lens, .landing__lens .sand-lens__hit, .w-orb .amara-lens, .amara-lens") as HTMLElement | null;
  const r = el?.getBoundingClientRect();
  const root = document.documentElement;
  root.style.setProperty("--orb-x", r ? `${r.left + r.width / 2}px` : "70%");
  root.style.setProperty("--orb-y", r ? `${r.top + r.height / 2}px` : "38%");
}

function setDoc(phase: "off" | "entering" | "on" | "exiting"): void {
  const root = document.documentElement;
  if (phase === "off") {
    root.removeAttribute("data-navy");
    root.removeAttribute("data-navy-phase");
    return;
  }
  root.setAttribute("data-navy-phase", phase === "on" ? "on" : phase);
  if (phase === "on" || phase === "exiting" || phase === "entering") root.setAttribute("data-navy", "on");
  if (phase === "on") root.removeAttribute("data-navy-phase");
}

function Glitch({ beat, reduce, t }: { beat: number; reduce: boolean; t: number }) {
  if (reduce) {
    return <div className="nv-fx" data-opt="reduce" aria-hidden="true" inert style={{ ["--navy-t" as string]: String(t) }}><div className="nv-fade" /></div>;
  }
  if (beat <= 0) return null;
  return (
    <div className="nv-fx" data-opt="glitch" data-beat={String(beat)} aria-hidden="true" inert>
      <div className="nv-tear" /><div className="nv-tear" /><div className="nv-tear" />
      <div className="nv-rgb" /><div className="nv-flash" /><div className="nv-end" />
    </div>
  );
}

/**
 * Session-only dark look. Mount once at the app root. The live flag lives in memory and
 * resets on a restart. `nexus.navyPreview` is a test/capture trigger only.
 */
export function NavyRoot({ children }: { children: ReactNode }) {
  const s = useNavyState();
  const frozen = PREVIEW_BEAT[readNavyPreview()?.scenario ?? ""] ?? 0;
  const [liveBeat, setBeat] = useState(0);
  const [fade, setFade] = useState(frozen >= 3 ? 1 : frozen ? frozen / 3 : 0);
  const beat = frozen || liveBeat;
  const holdRaf = useRef(0);

  useEffect(() => {
    const preview = readNavyPreview();
    if (preview) applyNavyPreview(preview);
    return () => setDoc("off");
  }, []);

  useEffect(() => {
    pinOrb();
    if (frozen) {
      setDoc("entering");
      if (frozen >= 3) document.documentElement.setAttribute("data-navy", "on");
      return undefined;
    }
    const reduce = reducedNavyMotion();
    if (s.phase === "on") { setDoc("on"); return undefined; }
    if (s.phase === "off") { setDoc("off"); return undefined; }
    const start = performance.now();
    const entering = s.phase === "entering";
    const dur = reduce ? REDUCE_MS : (entering ? ENTER_MS : EXIT_MS);
    if (entering) setDoc("entering");
    else setDoc("exiting");
    let id = 0;
    let fallback = 0;
    const tick = (now: number) => {
      const t = Math.min(1, Math.max(0, (now - start) / dur));
      pinOrb();
      if (reduce) {
        setFade(entering ? t : 1 - t);
        setBeat(0);
        if (entering && t > 0) document.documentElement.setAttribute("data-navy", "on");
      } else if (entering) {
        const ms = t * ENTER_MS;
        const next = ms < 150 ? 0 : ms < 380 ? 1 : ms < 750 ? 2 : 3;
        setBeat(next);
        setFade(t);
        if (ms >= 750) document.documentElement.setAttribute("data-navy", "on");
      } else {
        setFade(1 - t);
        setBeat(0);
      }
      if (t >= 1) {
        setBeat(0);
        if (entering) finishEnter();
        else finishExit();
        return;
      }
      id = requestAnimationFrame(tick);
    };
    id = requestAnimationFrame(tick);
    // jsdom/test rAF can stall; finish on a timer so reduced-motion and unit tests still settle.
    fallback = window.setTimeout(() => tick(start + dur), dur + 16);
    return () => { cancelAnimationFrame(id); window.clearTimeout(fallback); };
  }, [s.phase, frozen]);

  useEffect(() => {
    if (s.phase !== "on") return;
    const onDown = (event: PointerEvent) => {
      const t = event.target as Element | null;
      if (!t?.closest?.(".amara-lens, .amara-lens-wrap, .landing__lens, .w-orb")) return;
      if (navyStore.get().hold === "confirm") return;
      beginHold();
      const start = performance.now();
      const step = (now: number) => {
        const at = Math.min(1, (now - start) / HOLD_MS);
        setHoldAt(at);
        if (at >= 1) { reachHoldConfirm(); return; }
        holdRaf.current = requestAnimationFrame(step);
      };
      holdRaf.current = requestAnimationFrame(step);
    };
    const onUp = () => {
      cancelAnimationFrame(holdRaf.current);
      if (navyStore.get().hold === "holding") cancelHold();
    };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      cancelAnimationFrame(holdRaf.current);
    };
  }, [s.phase]);

  const reduce = typeof document !== "undefined" && reducedNavyMotion();
  const showFx = s.phase === "entering" || beat > 0 || (s.phase === "exiting" && reduce);

  return (
    <>
      {children}
      {showFx && <Glitch beat={beat} reduce={reduce} t={Math.min(1, Math.max(0, fade))} />}
      {s.caption && s.phase !== "off" && (
        <div className="nv-caption" role="status" aria-live="polite">
          <b>{s.caption.text}</b>
          {s.caption.sample && <span className="nv-tag"><DataTag kind="sample" />{s.caption.spoken ? " · spoken" : ""}</span>}
        </div>
      )}
      {s.hold === "holding" && <div className="nv-hold" aria-hidden="true" style={{ transform: `translate(-50%, -50%) rotate(${s.holdAt * 360}deg)` }} />}
      {s.hold === "confirm" && (
        <div className="nv-confirm" role="dialog" aria-label="Back to normal?">
          <span>Back to normal?</span>
          <button type="button" data-primary onClick={() => confirmHold()}>Confirm</button>
          <button type="button" onClick={() => cancelHold()}>Keep</button>
        </div>
      )}
    </>
  );
}
