import { useCallback, type ReactNode } from "react";
import { spanAxis } from "../MindBodyCharts";
import { alpha, type Prim, type RGBA } from "./prims";
import type { Theme } from "./theme";
import { useGfx } from "./useGfx";
import "./mindbody3d.css";

export type Stage = { label: string; minutes: number; tone: string };
const TRACK_Y = 3, TRACK_H = 10, AXIS_Y = 18, STRIP_H = 12, STRIP_GAP = 3;

export function stageWidths(stages: readonly Stage[], w: number, gap = STRIP_GAP): Array<{ tone: string; x: number; w: number }> {
  const total = stages.reduce((s, st) => s + Math.max(0, st.minutes), 0);
  const shown = stages.filter((st) => st.minutes > 0);
  if (total <= 0 || shown.length === 0) return [];
  const room = Math.max(0, w - gap * (shown.length - 1));
  let x = 0;
  return shown.map((st) => { const sw = (room * st.minutes) / total; const out = { tone: st.tone, x, w: sw }; x += sw + gap; return out; });
}

const stageColour = (tone: string, t: Theme): RGBA => (tone === "deep" ? t.deep : tone === "rem" ? t.rem : tone === "light" ? t.light : t.label3);

export function buildSleepNightPrims(w: number, h: number, span: { s: number; e: number; a0: number; a1: number } | null, stages: readonly Stage[], t: Theme): Prim[] {
  const out: Prim[] = [];
  if (span) {
    const px = (m: number) => ((m - span.a0) / (span.a1 - span.a0)) * w;
    out.push({ k: "rect", x: 0, y: TRACK_Y, w, h: TRACK_H, c: t.sep });
    out.push({ k: "rect", x: px(span.s), y: TRACK_Y, w: Math.max(2, px(span.e) - px(span.s)), h: TRACK_H, c: alpha(t.accent, 0.55), c1: t.accent });
  }
  const y = h - STRIP_H;
  for (const seg of stageWidths(stages, w)) {
    if (seg.tone === "awake") {
      out.push({ k: "rect", x: seg.x, y, w: seg.w, h: STRIP_H, c: alpha(t.label3, 0.18) });
      for (let sx = seg.x + 2; sx < seg.x + seg.w - 1; sx += 5) out.push({ k: "rect", x: sx, y: y + 1, w: Math.min(1.5, seg.x + seg.w - sx), h: STRIP_H - 2, c: alpha(t.label2, 0.7) });
    } else out.push({ k: "rect", x: seg.x, y, w: seg.w, h: STRIP_H, c: stageColour(seg.tone, t) });
  }
  return out;
}

/** Last night: bed→wake span and the stage strip (totals only), drawn in Three.js; `fallback` is the existing HTML. */
export function SleepNightGfx({ night, stages, fallback }: { night: { start: string; end: string } | null; stages: readonly Stage[]; fallback: ReactNode }) {
  const axis = night ? spanAxis(night.start, night.end) : null;
  const height = axis ? AXIS_Y + 32 + 6 + STRIP_H : STRIP_H;
  const draw = useCallback((w: number, h: number, t: Theme) => buildSleepNightPrims(w, h, axis, stages, t), [axis, stages]);
  const { hostRef, canvasRef, status } = useGfx(draw);
  if (status === "failed") return <>{fallback}</>;
  const text = [axis ? `In bed ${axis.startClock} to ${axis.endClock}` : null, ...stages.filter((s) => s.minutes > 0).map((s) => `${s.label} ${Math.round(s.minutes)} minutes`)].filter(Boolean).join(", ");
  return (
    <div ref={hostRef} className="gfx gfx--sleep" data-plot="" style={{ height }}>
      <canvas ref={canvasRef} className="gfx__canvas" aria-hidden="true" />
      {axis && (
        <div className="mb-span__axis" aria-hidden="true">
          {axis.hours.map((m) => <span key={m} style={{ left: `${axis.pct(m)}%` }}>{axis.label(m)}</span>)}
          <b style={{ left: `${axis.pct(axis.s)}%` }}>Bed {axis.startClock}</b>
          <b data-end="" style={{ left: `${axis.pct(axis.e)}%` }}>Wake {axis.endClock}</b>
        </div>
      )}
      <p className="mb-sr">{text}</p>
    </div>
  );
}
