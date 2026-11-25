/* eslint-disable @typescript-eslint/no-explicit-any -- `three` is untyped here (sandLens/three-shim.d.ts) */
import { useCallback, useMemo, useState, type ReactNode } from "react";
import { bandRows, placeWords, sectorAngles, shareHeight, type Range, type RingKey, type Shares } from "./neuroData";
import { glOk, useStage, type Rgb, type Stage, type StageCtx } from "./stage";
import "./three.css";

type Pt = { x: number; y: number };
type Hover = { i: number; x: number; y: number } | null;

const col = (THREE: any, c: Rgb) => new THREE.Color(c[0], c[1], c[2]);

/** The tilted 3D radial spectrum, delta to gamma: bar = this session's share, ghost slab = his usual range. Drawn on demand only (no continuous animation). */
function buildBrain(ctx: StageCtx, p: { shares: Shares; ranges: Record<RingKey, Range> | null }, io: { labels: (l: Pt[], c: Pt) => void; hover: (h: Hover) => void }): Stage {
  const { THREE, palette } = ctx;
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(28, 1, 0.1, 50);
  const group = new THREE.Group(); scene.add(group);
  scene.add(new THREE.AmbientLight(0xffffff, 0.7));
  const sun = new THREE.DirectionalLight(0xffffff, 1.1); sun.position.set(-2, 5, 3); scene.add(sun);
  const disposables: Array<{ dispose(): void }> = [];
  const own = <T extends { dispose(): void }>(o: T) => { disposables.push(o); return o; };
  const floor = new THREE.Mesh(own(new THREE.RingGeometry(0.42, 1.12, 96)), own(new THREE.MeshBasicMaterial({ color: col(THREE, palette.label), transparent: true, opacity: 0.06, side: THREE.DoubleSide })));
  floor.rotation.x = -Math.PI / 2; group.add(floor);
  const bars: any[] = [], mats: any[] = [];
  const rows = bandRows(p.shares, p.ranges);
  const lift = (i: number) => shareHeight(rows[i].share);
  rows.forEach((r, i) => {
    const { a0, a1 } = sectorAngles(i);
    const shape = new THREE.Shape();
    shape.absarc(0, 0, 1.0, a0, a1, false); shape.absarc(0, 0, 0.5, a1, a0, true); shape.closePath();
    const geo = own(new THREE.ExtrudeGeometry(shape, { depth: 1, bevelEnabled: false, curveSegments: 24 }));
    const mat = own(new THREE.MeshLambertMaterial({ color: col(THREE, palette.bands[i]), emissive: col(THREE, palette.bands[i]), emissiveIntensity: palette.dark ? 0.14 : 0.06 }));
    const bar = new THREE.Mesh(geo, mat); bar.rotation.x = -Math.PI / 2; bar.scale.z = Math.max(0.005, lift(i)); bar.userData.i = i; group.add(bar); bars.push(bar); mats.push(mat);
    if (r.usual) {
      const lo = shareHeight(r.usual.lo), hi = shareHeight(r.usual.hi);
      const gm = own(new THREE.MeshBasicMaterial({ color: col(THREE, palette.label), transparent: true, opacity: palette.dark ? 0.38 : 0.3, depthWrite: false, depthTest: false }));
      const ghost = new THREE.Mesh(geo, gm); ghost.rotation.x = -Math.PI / 2; ghost.position.y = lo; ghost.scale.z = Math.max(0.01, hi - lo); ghost.renderOrder = 2; group.add(ghost);
      const eg = own(new THREE.EdgesGeometry(geo, 30)), em = own(new THREE.LineBasicMaterial({ color: col(THREE, palette.label), transparent: true, opacity: 0.5, depthTest: false }));
      const edge = new THREE.LineSegments(eg, em); edge.rotation.x = -Math.PI / 2; edge.position.y = lo; edge.scale.z = Math.max(0.01, hi - lo); group.add(edge);
    }
  });
  const ray = new THREE.Raycaster(); let w = 1, h = 1, hot = -1;
  const project = (x: number, y: number, z: number): Pt => { const v = new THREE.Vector3(x, y, z).project(camera); return { x: (v.x * 0.5 + 0.5) * w, y: (-v.y * 0.5 + 0.5) * h }; };
  const place = () => {
    const elev = (58 * Math.PI) / 180, aspect = w / h, half = (14 * Math.PI) / 180;
    const need = Math.max(1.3 / Math.tan(half), 1.2 / (Math.tan(half) * aspect));
    camera.aspect = aspect; camera.position.set(0, Math.sin(elev) * need, Math.cos(elev) * need); camera.lookAt(0, 0.1, 0); camera.updateProjectionMatrix(); camera.updateMatrixWorld(true);
    io.labels(rows.map((_, i) => { const { a0, a1 } = sectorAngles(i), mid = (a0 + a1) / 2; return project(0.78 * Math.cos(mid), lift(i) + 0.06, -0.78 * Math.sin(mid)); }), project(0, 0.02, 0));
  };
  return {
    scene, camera,
    update() { mats.forEach((m, i) => { m.emissiveIntensity = hot === i ? 0.55 : palette.dark ? 0.14 : 0.06; }); },
    resize(nw, nh) { w = Math.max(1, nw); h = Math.max(1, nh); place(); },
    pointer(x, y) {
      const prev = hot;
      if (x == null || y == null) hot = -1;
      else { ray.setFromCamera(new THREE.Vector2((x / w) * 2 - 1, -((y / h) * 2 - 1)), camera); const hit = ray.intersectObjects(bars, false)[0]; hot = hit ? hit.object.userData.i : -1; }
      io.hover(hot >= 0 && x != null && y != null ? { i: hot, x, y } : null);
      if (prev !== hot) ctx.invalidate();
    },
    dispose() { disposables.forEach((d) => d.dispose()); scene.clear(); },
  };
}

export function NeuroBrainRing({ shares, ranges, usualCount, centre, onFail, children }: {
  shares: Shares; ranges: Record<RingKey, Range> | null; usualCount: number; centre: { date: string; duration: string }; onFail: () => void; children?: ReactNode;
}) {
  const rows = useMemo(() => bandRows(shares, ranges), [shares, ranges]);
  const [labels, setLabels] = useState<{ l: Pt[]; c: Pt } | null>(null);
  const [hover, setHover] = useState<Hover>(null);
  const sig = JSON.stringify([shares, ranges]);
  const build = useCallback((ctx: StageCtx) => buildBrain(ctx, { shares, ranges }, { labels: (l, c) => setLabels({ l, c }), hover: setHover }), [shares, ranges]);
  const { canvasRef } = useStage({ enabled: true, sig, animated: false, build, onFail });
  const hov = hover ? rows[hover.i] : null;
  return (
    <div className="d18-brain" data-testid="neuro-brain-ring">
      <div className="d18-brain__stage">
        <canvas ref={canvasRef} className="d18-brain__canvas" aria-hidden="true" />
        {labels && rows.map((r, i) => (
          <span key={r.key} className="d18-brain__lab" aria-hidden="true" data-place={r.place ?? undefined} style={{ left: labels.l[i].x, top: labels.l[i].y }}>
            <b>{r.name}</b> {Math.round(r.share)}%{r.place === "above" ? " ▲" : r.place === "below" ? " ▼" : ""}
          </span>
        ))}
        {labels && <span className="d18-brain__mid" aria-hidden="true" style={{ left: labels.c.x, top: labels.c.y }}><small>{centre.date}</small><b>{centre.duration}</b></span>}
        {hov && hover && (
          <span className="d18-brain__tip" aria-hidden="true" style={{ left: hover.x, top: hover.y }}>
            <b>{hov.name}</b> {hov.range}<br />{hov.share.toFixed(1)}% of session power
            <small>{hov.usual ? `Usual ${Math.round(hov.usual.lo)} to ${Math.round(hov.usual.hi)}% · ${placeWords(hov.place)}` : "Usual range needs 3 sessions with band data"}</small>
          </span>
        )}
      </div>
      <BrainText rows={rows} usualCount={usualCount} centre={centre} />
      {children}
    </div>
  );
}

/** The same facts as the ring, as a table (visually hidden) plus the one-line legend that is visible. */
export function BrainText({ rows, usualCount, centre }: { rows: ReturnType<typeof bandRows>; usualCount: number; centre: { date: string; duration: string } }) {
  const hasUsual = rows.some((r) => r.usual);
  return (
    <>
      <table className="d18-vh">
        <caption>Brainwave bands of your last session, {centre.date}, {centre.duration}{hasUsual ? `. Usual range from your last ${usualCount} sessions with band data` : ""}</caption>
        <thead><tr><th scope="col">Band</th><th scope="col">Frequency</th><th scope="col">Share of session power</th><th scope="col">Your usual</th></tr></thead>
        <tbody>{rows.map((r) => (
          <tr key={r.key}><th scope="row">{r.name}</th><td>{r.range}</td><td>{r.share.toFixed(1)}%</td><td>{r.usual ? `${r.usual.lo} to ${r.usual.hi}% · ${placeWords(r.place)}` : "not enough sessions yet"}</td></tr>
        ))}</tbody>
      </table>
      <p className="d18-note">{hasUsual ? `Bar height is the share of band power; the grey slab is your usual range from your last ${usualCount} sessions with band data.` : `Bar height is the share of band power. Your usual range appears once ${3} sessions have band data (${usualCount} so far).`}</p>
    </>
  );
}

export const brainGlAvailable = glOk;
