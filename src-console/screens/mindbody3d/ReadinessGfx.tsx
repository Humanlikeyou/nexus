import { useCallback, useRef, useState, type ReactNode } from "react";
import { lean, type MetricReading } from "../../lib/mindBodyDeep";
import { DeltaBar } from "../MindBodyCharts";
import { deltaReach, usualZoneHalf } from "./geometry";
import { alpha, type Prim } from "./prims";
import type { Theme } from "./theme";
import { useGfx } from "./useGfx";
import "./mindbody3d.css";

const DASH = "—";
export type RowTrack = { left: number; width: number; cy: number };

/** Design A: per driver, the usual zone (±1 sd), a hairline track, the centre tick and a bar out to its z-score. */
export function buildReadinessPrims(tracks: readonly RowTrack[], items: ReadonlyArray<{ z: number | null; lean: "up" | "down" | "flat" | string }>, hover: number | null, t: Theme): Prim[] {
  const out: Prim[] = [];
  tracks.forEach((tr, i) => {
    const item = items[i];
    if (!item) return;
    const half = tr.width / 2, cx = tr.left + half, zone = usualZoneHalf(half);
    if (hover === i) out.push({ k: "rect", x: tr.left - 8, y: tr.cy - 14, w: tr.width + 16, h: 28, c: alpha(t.label, 0.05) });
    out.push({ k: "rect", x: cx - zone, y: tr.cy - 5, w: zone * 2, h: 10, c: alpha(t.accent, t.dark ? 0.16 : 0.1) });
    out.push({ k: "rect", x: tr.left, y: tr.cy - 1, w: tr.width, h: 2, c: alpha(t.label, 0.12) });
    out.push({ k: "rect", x: cx - 0.5, y: tr.cy - 6, w: 1, h: 12, c: t.label3 });
    if (item.z === null) return;
    const reach = Math.max(1, deltaReach(item.z, half)), side = item.z >= 0 ? 1 : -1;
    const col = item.lean === "up" ? t.accent : item.lean === "down" ? t.copper : t.label2;
    const x = side > 0 ? cx : cx - reach;
    out.push({ k: "rect", x, y: tr.cy - 4, w: reach, h: 8, c: alpha(col, t.dark ? 0.22 : 0.14) }, { k: "rect", x, y: tr.cy - 1.3, w: reach, h: 2.6, c: col }, { k: "dot", x: cx + side * reach, y: tr.cy, r: 3.6, c: col });
  });
  return out;
}

const sdWords = (z: number) => (Math.abs(z) < 0.05 ? "at your usual" : `${Math.abs(z).toFixed(1)} sd ${z > 0 ? "above" : "below"} your usual`);

/**
 * The drivers list. The list itself (labels, values, usual) is unchanged HTML; with WebGL the delta bars are
 * drawn by Three.js on one canvas behind it, aligned to each row's track. Without WebGL it is the old DeltaBar.
 */
export function ReadinessDrivers({ items }: { items: readonly MetricReading[] }) {
  const wrap = useRef<HTMLDivElement>(null);
  const [hoverRow, setHoverRow] = useState<{ i: number; top: number } | null>(null);
  const hover = hoverRow?.i ?? null;
  const draw = useCallback((_w: number, _h: number, theme: Theme) => {
    const root = wrap.current;
    if (!root) return [];
    const box = root.getBoundingClientRect();
    const tracks = Array.from(root.querySelectorAll(".mb-delta")).map((el) => { const r = el.getBoundingClientRect(); return { left: r.left - box.left, width: r.width, cy: r.top - box.top + r.height / 2 }; });
    return buildReadinessPrims(tracks, items.map((it) => ({ z: it.z, lean: lean(it) })), hover, theme);
  }, [items, hover]);
  const { hostRef, canvasRef, status } = useGfx(draw);
  const gfx = status === "ready";
  const list: ReactNode = (
    <ul className="mb-drivers" aria-label="What's driving today" data-gfx={gfx || undefined}>
      {items.map((item, i) => (
        <li key={item.def.key} data-lean={lean(item)} onPointerEnter={(e) => setHoverRow({ i, top: e.currentTarget.offsetTop })} onPointerLeave={() => setHoverRow(null)}>
          <span className="mb-drivers__label">{item.def.label}</span>
          <span className="mb-drivers__value">{item.value === null ? DASH : item.def.format(item.value)}</span>
          <DeltaBar reading={item} />
          <span className="mb-drivers__usual">{item.base ? `usual ${item.def.format(item.base.mean)}` : "no baseline yet"}</span>
          {gfx && item.z !== null && <span className="mb-sr">{sdWords(item.z)}</span>}
        </li>
      ))}
    </ul>
  );
  if (status === "failed") return list;
  const hv = hover != null ? items[hover] : null;
  return (
    <div ref={wrap} className="mb-driversgfx">
      <div ref={hostRef} className="gfx" data-fill="" aria-hidden="true"><canvas ref={canvasRef} className="gfx__canvas" /></div>
      {list}
      {gfx && hv && hv.base && hv.z !== null && (
        <div className="mb-driversgfx__tip" aria-hidden="true" style={{ right: 0, top: (hoverRow?.top ?? 0) + 36 }}>
          <strong>{hv.def.label} {hv.value === null ? DASH : hv.def.format(hv.value)}</strong>
          <span>Usual {hv.def.format(hv.base.mean - hv.base.sd)} to {hv.def.format(hv.base.mean + hv.base.sd)} · {sdWords(hv.z)}</span>
        </div>
      )}
    </div>
  );
}
