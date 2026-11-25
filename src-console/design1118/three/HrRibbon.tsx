/* eslint-disable @typescript-eslint/no-explicit-any -- `three` is untyped here (sandLens/three-shim.d.ts) */
import { useCallback, useState } from "react";
import { tokyoTime, type HrWindow } from "../../hrTake/adapters";
import { aboveUsual, gapLabel, hrFacts, hrShape, type HrShape } from "./hrData";
import { makeRibbon, useStage, type Rgb, type Stage, type StageCtx } from "./stage";
import "./three.css";

const PAD_X = 5, PAD_T = 6, PAD_B = 4;
type Hover = { i: number; bpm: number } | null;

function buildHr(ctx: StageCtx, s: HrShape, io: { hover: (h: Hover) => void }): Stage {
  const { THREE, palette } = ctx;
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(0, 1, 0, 1, -1, 1); // y grows downward, in CSS px
  const own: Array<{ dispose(): void }> = [];
  const mat = (o: any) => { const m = new THREE.MeshBasicMaterial({ transparent: true, depthTest: false, side: THREE.DoubleSide, ...o }); own.push(m); return m; };
  const rgb = (c: Rgb) => new THREE.Color(c[0], c[1], c[2]);
  const glowBlend = palette.dark ? THREE.AdditiveBlending : THREE.NormalBlending;
  const bandGeo = new THREE.PlaneGeometry(1, 1); own.push(bandGeo);
  const band = s.usual ? new THREE.Mesh(bandGeo, mat({ color: rgb(palette.accent), opacity: palette.dark ? 0.16 : 0.12 })) : null;
  if (band) scene.add(band);
  const edges = s.usual ? [s.usual.hi, s.usual.lo].map(() => { const g = new THREE.BufferGeometry().setAttribute("position", new THREE.BufferAttribute(new Float32Array(6), 3)); const m = new THREE.LineBasicMaterial({ color: rgb(palette.accent), transparent: true, opacity: 0.45, depthTest: false }); own.push(g, m); const l = new THREE.Line(g, m); l.frustumCulled = false; scene.add(l); return l; }) : [];
  const runs = s.runs.map((run) => {
    const core = makeRibbon(THREE, run.length, mat({ vertexColors: true, opacity: 1 }));
    const glow = makeRibbon(THREE, run.length, mat({ vertexColors: true, opacity: palette.dark ? 0.22 : 0.16, blending: glowBlend }));
    const ag = new THREE.BufferGeometry(); const ap = new Float32Array(run.length * 6); ag.setAttribute("position", new THREE.BufferAttribute(ap, 3));
    const idx: number[] = []; for (let k = 0; k < run.length - 1; k++) { const a = k * 2; idx.push(a, a + 1, a + 2, a + 1, a + 3, a + 2); } ag.setIndex(idx);
    const area = new THREE.Mesh(ag, mat({ color: rgb(palette.accent), opacity: palette.dark ? 0.14 : 0.1 })); area.frustumCulled = false;
    scene.add(area, glow.mesh, core.mesh); own.push(core.geo, glow.geo, ag);
    return { run, core, glow, ag, ap };
  });
  const bridges = s.gaps.map((g, k) => {
    const geo = new THREE.BufferGeometry().setAttribute("position", new THREE.BufferAttribute(new Float32Array(6), 3));
    const m = new THREE.LineDashedMaterial({ color: rgb(palette.label2), dashSize: 4, gapSize: 4, transparent: true, opacity: 0.85, depthTest: false }); own.push(geo, m);
    const l = new THREE.Line(geo, m); l.frustumCulled = false; scene.add(l); return { l, geo, g, k };
  });
  const dotGeo = new THREE.CircleGeometry(4, 24), ringGeo = new THREE.RingGeometry(0.86, 1, 40); own.push(dotGeo, ringGeo);
  const last = s.last;
  const lastCol = last && aboveUsual(last.bpm, s) ? palette.copper : palette.accent;
  const dot = last ? new THREE.Mesh(dotGeo, mat({ color: rgb(lastCol), opacity: 1 })) : null;
  const ring = last ? new THREE.Mesh(ringGeo, mat({ color: rgb(lastCol), opacity: 0.6 })) : null;
  if (dot && ring) scene.add(ring, dot);
  let w = 1, h = 1;
  const X = (i: number) => PAD_X + (i * (w - 2 * PAD_X)) / s.n;
  const Y = (v: number) => PAD_T + (h - PAD_T - PAD_B) * (1 - (v - s.lo) / Math.max(1, s.hi - s.lo));
  const colorOf = (bpm: number): Rgb => (aboveUsual(bpm, s) ? palette.copper : palette.accent);
  const layout = () => {
    camera.left = 0; camera.right = w; camera.top = 0; camera.bottom = h; camera.updateProjectionMatrix();
    if (band && s.usual) { band.scale.set(w, Math.max(1, Y(s.usual.lo) - Y(s.usual.hi)), 1); band.position.set(w / 2, (Y(s.usual.lo) + Y(s.usual.hi)) / 2, 0); }
    edges.forEach((l, k) => { const y = Y(k === 0 ? s.usual!.hi : s.usual!.lo); const a = l.geometry.attributes.position; a.setXYZ(0, 0, y, 0); a.setXYZ(1, w, y, 0); a.needsUpdate = true; });
    runs.forEach(({ run, core, glow, ag, ap }) => {
      const pts = run.map((p) => { const c = colorOf(p.bpm); return [X(p.i), Y(p.bpm), c[0], c[1], c[2]]; });
      if (pts.length > 1) { core.set(pts, 2.5); glow.set(pts, 7); } else { core.set([], 2); glow.set([], 6); }
      run.forEach((p, k) => ap.set([X(p.i), Y(p.bpm), 0, X(p.i), h - PAD_B, 0], k * 6));
      ag.attributes.position.needsUpdate = true; ag.setDrawRange(0, Math.max(0, (run.length - 1) * 6));
    });
    bridges.forEach(({ l, geo, g }) => {
      const a = s.runs.flat().find((p) => p.i === g.from)!, b = s.runs.flat().find((p) => p.i === g.to)!;
      const pos = geo.attributes.position; pos.setXYZ(0, X(a.i), Y(a.bpm), 0); pos.setXYZ(1, X(b.i), Y(b.bpm), 0); pos.needsUpdate = true; l.computeLineDistances();
    });
    if (last && dot && ring) { dot.position.set(X(last.i), Y(last.bpm), 0.1); ring.position.set(X(last.i), Y(last.bpm), 0.1); }
  };
  const pts = s.runs.flat();
  return {
    scene, camera,
    update(t, still) { if (!ring) return; const k = still ? 0.5 : (t % 1.8) / 1.8; ring.scale.set(5 + k * 14, 5 + k * 14, 1); ring.material.opacity = 0.7 * (1 - k); },
    resize(nw, nh) { w = Math.max(1, nw); h = Math.max(1, nh); layout(); },
    pointer(x) {
      if (x == null || !pts.length) { io.hover(null); return; }
      let best = pts[0]; for (const p of pts) if (Math.abs(X(p.i) - x) < Math.abs(X(best.i) - x)) best = p;
      io.hover({ i: best.i, bpm: best.bpm });
    },
    dispose() { own.forEach((o) => o.dispose()); scene.clear(); },
  };
}

const at = (i: number, n: number) => `calc(${PAD_X}px + (100% - ${2 * PAD_X}px) * ${i / n})`;

/** The take graph in WebGL: a glowing ribbon over the usual band (copper above usual), a dashed bridge across gaps, a pulsing last point. */
export function HrTraceGL({ w, label, onFail }: { w: HrWindow; label: string; onFail: () => void }) {
  const s = hrShape(w);
  const [hover, setHover] = useState<Hover>(null);
  const sig = JSON.stringify([w.perMinute.map((m) => m.bpm), w.usual]);
  const build = useCallback((ctx: StageCtx) => buildHr(ctx, s, { hover: setHover }), [s]);
  const { canvasRef } = useStage({ enabled: true, sig, animated: true, build, onFail });
  const hp = hover ? w.perMinute[hover.i] : null;
  return (
    <>
      <div className="hrt-gl" role="img" aria-label={label} data-testid="hr-trace-gl">
        <canvas ref={canvasRef} className="hrt-gl__canvas" />
        {s.gaps.map((g) => <span key={g.from} className="hrt-gl__gap" style={{ left: at((g.from + g.to) / 2, s.n) }}>{gapLabel(g)}</span>)}
        {hover && hp && (
          <>
            <i className="hrt-gl__cross" style={{ left: at(hover.i, s.n) }} />
            <span className="hrt-gl__tip" style={{ left: at(hover.i, s.n) }} data-flip={hover.i > s.n / 2 || undefined}>
              <b>{hover.bpm} bpm</b> · {tokyoTime(hp.t)}
              {s.usual && <small>Usual {s.usual.lo} to {s.usual.hi} · {aboveUsual(hover.bpm, s) ? "above usual" : hover.bpm < s.usual.lo ? "below usual" : "inside usual"}</small>}
            </span>
          </>
        )}
      </div>
      <div className="d18-vh">
        {hrFacts(w, s).map((f) => <p key={f}>{f}</p>)}
        <table>
          <caption>Heart rate per minute, last {w.perMinute.length} minutes</caption>
          <thead><tr><th scope="col">Time</th><th scope="col">bpm</th></tr></thead>
          <tbody>{w.perMinute.map((m) => <tr key={m.t}><td>{tokyoTime(m.t)}</td><td>{m.bpm ?? "No signal"}</td></tr>)}</tbody>
        </table>
      </div>
    </>
  );
}
