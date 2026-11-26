/* eslint-disable @typescript-eslint/no-explicit-any -- `three` ships no types here (sandLens/three-shim.d.ts) */
import { useEffect, useRef, type RefObject } from "react";
import { webgl2Available } from "../../sandLens/gl";

/** Shared plumbing for the Three.js charts: lazy `three`, WebGL2 gate, DPR cap, pause when hidden, reduced motion, theme re-read, dispose. */

let probe: boolean | null = null;
/** WebGL2 is probed once per page; jsdom and old WebViews say no, so the SVG/text rendering stays. */
export const glOk = (): boolean => (probe ??= webgl2Available());
export const _resetGlProbe = () => { probe = null; };

export type Rgb = [number, number, number];
export type Palette = {
  dark: boolean;
  accent: Rgb; copper: Rgb; label: Rgb; label2: Rgb;
  /** Delta to gamma: copper to emerald, as in the approved mock-up. */
  bands: Rgb[];
};

function parse(css: string): Rgb | null {
  try {
    const c = document.createElement("canvas"); c.width = c.height = 1;
    const g = c.getContext("2d", { willReadFrequently: true }); if (!g) return null;
    g.clearRect(0, 0, 1, 1); g.fillStyle = "#000"; g.fillStyle = css; g.fillRect(0, 0, 1, 1);
    const [r, gg, b, a] = g.getImageData(0, 0, 1, 1).data;
    return a === 0 ? null : [r / 255, gg / 255, b / 255];
  } catch { return null; }
}
const FALLBACK: Rgb = [0.5, 0.5, 0.5];
const mix = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const lum = (c: Rgb) => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];

/** Reads the design tokens (CSS variables) as the element sees them right now. */
export function readPalette(el: HTMLElement): Palette {
  const span = document.createElement("span"); span.style.display = "none"; el.appendChild(span);
  const get = (v: string, fb: Rgb = FALLBACK): Rgb => { span.style.color = ""; span.style.color = `var(${v})`; return parse(getComputedStyle(span).color) ?? fb; };
  const accent = get("--accent-strong"), copper = get("--copper"), label = get("--label"), label2 = get("--label-secondary");
  span.remove();
  const dark = lum(label) > 0.5;
  // Light theme: the emerald end is --accent-strong (dark), so it stays readable on white; dark theme lifts it a little.
  const bands = [0, 1, 2, 3, 4].map((i) => mix(copper, dark ? mix(accent, [1, 1, 1], 0.35) : accent, i / 4));
  return { dark, accent: dark ? mix(accent, [1, 1, 1], 0.25) : accent, copper, label, label2, bands };
}

export type StageCtx = { THREE: any; canvas: HTMLCanvasElement; width: number; height: number; palette: Palette; invalidate: () => void };
export interface Stage {
  scene: any; camera: any;
  /** t in seconds since mount. Called on demand (still) or every visible frame (animated), just before the render. */
  update(t: number, still: boolean): void;
  resize(w: number, h: number): void;
  /** Pointer in CSS px relative to the canvas, or null when it leaves. */
  pointer?(x: number | null, y: number | null): void;
  dispose(): void;
}

export const reducedMotion = () => typeof window !== "undefined" && !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

export function useStage(opts: { enabled: boolean; sig: string; animated: boolean; build: (ctx: StageCtx) => Stage; onFail: () => void }): { canvasRef: RefObject<HTMLCanvasElement | null> } {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const live = useRef(opts);
  useEffect(() => { live.current = opts; });
  const { enabled, sig, animated } = opts;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!enabled || !canvas) return;
    let gone = false, cleanup = () => {};
    import("three").then((THREE) => {
      if (gone) return;
      const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, premultipliedAlpha: true });
      renderer.setClearColor(0x000000, 0);
      let w = canvas.clientWidth || 300, h = canvas.clientHeight || 150;
      let raf = 0, visible = true, stage: Stage | null = null, queued = false;
      const t0 = performance.now();
      const mq = window.matchMedia?.("(prefers-reduced-motion: reduce)");
      const still = () => !animated || !!mq?.matches;
      const draw = () => { if (stage) { stage.update((performance.now() - t0) / 1000, still()); renderer.render(stage.scene, stage.camera); } };
      const invalidate = () => { if (queued || gone) return; queued = true; requestAnimationFrame(() => { queued = false; if (!gone) draw(); }); };
      const loop = () => { raf = 0; if (gone || still()) return; if (visible && !document.hidden) draw(); raf = requestAnimationFrame(loop); };
      const kick = () => { if (!raf && !still()) raf = requestAnimationFrame(loop); else if (still()) invalidate(); };
      const size = () => {
        w = canvas.clientWidth || w; h = canvas.clientHeight || h;
        renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1));
        renderer.setSize(w, h, false);
        stage?.resize(w, h); invalidate();
      };
      const mount = () => {
        stage?.dispose();
        stage = live.current.build({ THREE, canvas, width: w, height: h, palette: readPalette(canvas.parentElement ?? canvas), invalidate });
        stage.resize(w, h); invalidate(); kick();
      };
      renderer.setPixelRatio(Math.min(2, window.devicePixelRatio || 1)); renderer.setSize(w, h, false);
      mount();
      const ro = new ResizeObserver(size); ro.observe(canvas);
      const io = new IntersectionObserver((es) => { visible = es.some((e) => e.isIntersecting); if (visible) kick(); }); io.observe(canvas);
      const onVis = () => { if (!document.hidden) kick(); };
      document.addEventListener("visibilitychange", onVis);
      // Theme: re-read the tokens when the theme attribute/class or the system scheme changes.
      const mo = new MutationObserver(mount); mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "class", "style"] });
      const dq = window.matchMedia?.("(prefers-color-scheme: dark)");
      dq?.addEventListener?.("change", mount); mq?.addEventListener?.("change", kick);
      const move = (e: PointerEvent) => { const r = canvas.getBoundingClientRect(); stage?.pointer?.(e.clientX - r.left, e.clientY - r.top); };
      const leave = () => stage?.pointer?.(null, null);
      canvas.addEventListener("pointermove", move); canvas.addEventListener("pointerleave", leave);
      cleanup = () => {
        cancelAnimationFrame(raf); ro.disconnect(); io.disconnect(); mo.disconnect();
        document.removeEventListener("visibilitychange", onVis); dq?.removeEventListener?.("change", mount); mq?.removeEventListener?.("change", kick);
        canvas.removeEventListener("pointermove", move); canvas.removeEventListener("pointerleave", leave);
        stage?.dispose(); stage = null; renderer.dispose();
      };
    }).catch((e) => { console.warn("[three] falling back to the flat chart", e); if (!gone) live.current.onFail(); });
    return () => { gone = true; cleanup(); };
  }, [enabled, sig, animated]);

  return { canvasRef };
}

/** A ribbon: a triangle strip along a polyline, rewritten in place (no per-frame allocation). */
export function makeRibbon(THREE: any, maxPts: number, mat: any) {
  const geo = new THREE.BufferGeometry();
  const pos = new Float32Array(maxPts * 2 * 3), col = new Float32Array(maxPts * 2 * 3);
  geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  geo.setAttribute("color", new THREE.BufferAttribute(col, 3));
  const idx: number[] = [];
  for (let s = 0; s < maxPts - 1; s++) { const a = s * 2; idx.push(a, a + 1, a + 2, a + 1, a + 3, a + 2); }
  geo.setIndex(idx);
  const mesh = new THREE.Mesh(geo, mat); mesh.frustumCulled = false;
  return {
    mesh, geo,
    /** pts: [x, y, r, g, b]. width in px. `closed` joins the last point back to the first. */
    set(pts: ReadonlyArray<readonly number[]>, width: number) {
      const n = Math.min(maxPts, pts.length);
      for (let i = 0; i < n; i++) {
        const p = pts[i], a = pts[Math.max(0, i - 1)], b = pts[Math.min(n - 1, i + 1)];
        let dx = b[0] - a[0], dy = b[1] - a[1]; const l = Math.hypot(dx, dy) || 1; dx /= l; dy /= l;
        const nx = -dy * width / 2, ny = dx * width / 2;
        pos.set([p[0] + nx, p[1] + ny, 0, p[0] - nx, p[1] - ny, 0], i * 6);
        col.set([p[2], p[3], p[4], p[2], p[3], p[4]], i * 6);
      }
      geo.setDrawRange(0, Math.max(0, (n - 1) * 6));
      geo.attributes.position.needsUpdate = true; geo.attributes.color.needsUpdate = true;
    },
  };
}
