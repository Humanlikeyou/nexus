import { useEffect, useRef, useState } from "react";
import { acquireShared, gfxSupported, paint, releaseShared } from "./shared";
import type { Prim } from "./prims";
import { readTheme, type Theme } from "./theme";

export type GfxStatus = "pending" | "ready" | "failed";
export type DrawFn = (w: number, h: number, theme: Theme) => Prim[];

/**
 * Wires one chart to the shared renderer. It:
 *  - starts "failed" when WebGL2 is missing, and turns "failed" if three or the context breaks (callers render their old SVG/HTML);
 *  - paints on demand only (no animation loop, so reduced motion is the same static picture);
 *  - skips painting while scrolled out of view or the page is hidden, and paints when it comes back;
 *  - repaints on resize and when the theme variables change;
 *  - releases its reference on the renderer on unmount.
 * DPR is capped at 2.
 */
export function useGfx(draw: DrawFn | null) {
  const hostRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [status, setStatus] = useState<GfxStatus>(() => (gfxSupported() ? "pending" : "failed"));
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [visible, setVisible] = useState(true);
  const [themeVersion, setThemeVersion] = useState(0);
  const sharedRef = useRef<Awaited<ReturnType<typeof acquireShared>>>(null);
  const themeRef = useRef<{ theme: Theme; version: number } | null>(null);

  useEffect(() => {
    if (!gfxSupported()) return;
    let dead = false;
    void acquireShared().then((s) => {
      if (dead) { if (s) releaseShared(); return; }
      if (!s) { setStatus("failed"); return; }
      sharedRef.current = s;
      setStatus("ready");
    });
    return () => { dead = true; if (sharedRef.current) { sharedRef.current = null; releaseShared(); } };
  }, []);

  useEffect(() => {
    const el = hostRef.current;
    if (!el) return;
    const cleanups: Array<() => void> = [];
    if (typeof ResizeObserver !== "undefined") {
      const ro = new ResizeObserver((entries) => { const r = entries[0]?.contentRect; if (r) setSize((p) => (Math.abs(p.w - r.width) < 0.5 && Math.abs(p.h - r.height) < 0.5 ? p : { w: r.width, h: r.height })); });
      ro.observe(el); cleanups.push(() => ro.disconnect());
    }
    if (typeof IntersectionObserver !== "undefined") {
      const io = new IntersectionObserver((entries) => { const e = entries[entries.length - 1]; if (e) setVisible(e.isIntersecting); });
      io.observe(el); cleanups.push(() => io.disconnect());
    }
    const bump = () => setThemeVersion((v) => v + 1);
    const mo = new MutationObserver(bump);
    mo.observe(document.documentElement, { attributes: true });
    cleanups.push(() => mo.disconnect());
    const mq = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;
    mq?.addEventListener?.("change", bump); cleanups.push(() => mq?.removeEventListener?.("change", bump));
    document.addEventListener("visibilitychange", bump); cleanups.push(() => document.removeEventListener("visibilitychange", bump));
    return () => cleanups.forEach((fn) => fn());
  }, [status]);

  useEffect(() => {
    const s = sharedRef.current, host = hostRef.current, canvas = canvasRef.current;
    if (status !== "ready" || !s || !host || !canvas || !draw || !visible || size.w < 2 || size.h < 2) return;
    if (typeof document !== "undefined" && document.hidden) return;
    const cached = themeRef.current;
    let theme = cached && cached.version === themeVersion ? cached.theme : null;
    if (!theme) { theme = readTheme(host); themeRef.current = { theme, version: themeVersion }; }
    const dpr = Math.min(2, typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1);
    if (!paint(s, canvas, size.w, size.h, dpr, draw(size.w, size.h, theme))) void Promise.resolve().then(() => setStatus("failed"));
  }, [status, size, visible, draw, themeVersion]);

  return { hostRef, canvasRef, status, size };
}
