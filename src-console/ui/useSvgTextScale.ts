import { useCallback, useRef } from "react";

/**
 * Charts drawn in viewBox units scale their text with the chart's width (11px could render at 9 or 15). This keeps
 * SVG text at its true CSS size: it measures the rendered scale and sets `--svg-u` (1 / scale) on the <svg>, and
 * the chart's CSS sizes text as `calc(11px * var(--svg-u, 1))`. Taisun 2026-11-19: all text on the φ scale.
 */
export function useSvgTextScale<T extends SVGSVGElement>(): (el: T | null) => void {
  const ro = useRef<ResizeObserver | null>(null);
  return useCallback((el: T | null) => {
    ro.current?.disconnect();
    ro.current = null;
    if (!el) return;
    const apply = () => {
      const vb = el.viewBox?.baseVal;
      const w = el.getBoundingClientRect().width;
      if (!vb || !vb.width || !w) return;
      el.style.setProperty("--svg-u", String(vb.width / w));
    };
    apply();
    if (typeof ResizeObserver !== "undefined") { ro.current = new ResizeObserver(apply); ro.current.observe(el); }
  }, []);
}
