import { useId, type ReactNode } from "react";
import type { Contact } from "./recordings";
import "./Recordings.css";

/** design-1110e E2/E4/E9/E11: stacked per-channel traces on one shared scale, hairline grid, contact as shape + word. */
export interface LaneSpec { key: string; name: ReactNode; label: string; where: string; color: string; values: number[]; contact?: Contact; note?: string; range?: number }
const CONTACT_WORD: Record<Contact, string> = { good: "Good contact", fair: "Fair contact", poor: "Poor contact", none: "No signal" };

/** Offset removal before drawing: a lane shows its swing around its own mean (1 Hz high-pass stand-in). */
export const centered = (values: number[]) => { if (!values.length) return values; const m = values.reduce((a, b) => a + b, 0) / values.length; return values.map((v) => v - m); };

function tracePath(values: number[], h: number, range: number) {
  const n = values.length; if (n < 2) return "";
  const half = h / 2 - 3;
  return values.map((v, i) => `${i ? "L" : "M"}${((i / (n - 1)) * 1000).toFixed(1)},${(h / 2 - (Math.max(-range, Math.min(range, v)) / range) * half).toFixed(1)}`).join("");
}

export function SignalLanes({ lanes, ticks, height = 56, range = 100, playhead, fade = false, scaleLabel = "50 µV", caption }: { lanes: LaneSpec[]; ticks: string[]; height?: number; range?: number; playhead?: number | null; fade?: boolean; scaleLabel?: string | null; caption: string }) {
  const gid = useId().replace(/:/g, "");
  const gridX = ticks.map((_, i) => (i / Math.max(1, ticks.length - 1)) * 1000);
  return (
    <div className="rec-lanes-wrap">
      <div className="rec-lanes">
        {lanes.map((lane, idx) => {
          const r = lane.range ?? range;
          return (
            <div className="rec-lane" key={lane.key} style={{ ["--c" as string]: lane.color }}>
              <div className="rec-lane__label" style={{ height }}>
                <span className="rec-lane__name"><i aria-hidden="true" />{lane.name}</span>
                <span className="rec-lane__where">{lane.where}</span>
                {lane.contact ? <span className="rec-q" data-q={lane.contact}>{CONTACT_WORD[lane.contact]}</span> : lane.note ? <span className="rec-lane__where">{lane.note}</span> : null}
              </div>
              <div className="rec-lane__plot">
                <svg className="rec-plot" viewBox={`0 0 1000 ${height}`} preserveAspectRatio="none" height={height} role="img" aria-label={`${lane.label}, ${caption}`}>
                  {gridX.map((x) => <line key={x} className="rec-plot__g" x1={x} x2={x} y1={0} y2={height} vectorEffect="non-scaling-stroke" />)}
                  <line className="rec-plot__z" x1={0} x2={1000} y1={height / 2} y2={height / 2} vectorEffect="non-scaling-stroke" />
                  <path className="rec-plot__tr" data-q={lane.contact} d={tracePath(lane.values, height, r)} vectorEffect="non-scaling-stroke" />
                  {playhead != null && <line className="rec-plot__ph" x1={playhead * 1000} x2={playhead * 1000} y1={0} y2={height} vectorEffect="non-scaling-stroke" />}
                  {fade && <><defs><linearGradient id={`${gid}f`} x1="0" x2="1"><stop offset="0" style={{ stopColor: "var(--bg-content, #fff)", stopOpacity: 0.9 }} /><stop offset="1" style={{ stopColor: "var(--bg-content, #fff)", stopOpacity: 0 }} /></linearGradient></defs><rect x={0} y={0} width={70} height={height} fill={`url(#${gid}f)`} /></>}
                </svg>
                {idx === 0 && scaleLabel && <span className="rec-scale" aria-hidden="true"><i style={{ height: (height / 2 - 3) / 2 }} />{scaleLabel}</span>}
              </div>
            </div>
          );
        })}
      </div>
      <div className="rec-axis" aria-hidden="true"><span /><span className="rec-axis__t">{ticks.map((t, i) => <span key={`${t}-${i}`}>{t}</span>)}</span></div>
    </div>
  );
}
