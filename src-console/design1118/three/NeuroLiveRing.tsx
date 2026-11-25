import { useCallback, useEffect, useRef, useState } from "react";
import { bandRows, bandShares, placeWords, RING_BANDS, type Range, type RingKey, type Shares } from "./neuroData";
import { glOk, makeRibbon, useStage, type Rgb, type Stage, type StageCtx } from "./stage";
import "./three.css";

const KF = [2, 3, 5, 8, 13];
const N = 160;
type Hover = { i: number; x: number; y: number } | null;

/** Five concentric loops, one per band: wave height = live share, soft halo = his usual envelope. */
function buildLive(ctx: StageCtx, live: { current: Shares | null }, ranges: Record<RingKey, Range> | null, io: { hover: (h: Hover) => void }): Stage {
  const { THREE, palette } = ctx;
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(0, 1, 1, 0, -1, 1);
  const own: Array<{ dispose(): void }> = [];
  const mk = (op: number, glow = false) => { const m = new THREE.MeshBasicMaterial({ vertexColors: true, transparent: true, opacity: op, depthTest: false, side: THREE.DoubleSide, blending: glow && palette.dark ? THREE.AdditiveBlending : THREE.NormalBlending }); own.push(m); return m; };
  const rings = RING_BANDS.map((b, i) => {
    const env = makeRibbon(THREE, N + 1, mk(palette.dark ? 0.12 : 0.14)), glow = makeRibbon(THREE, N + 1, mk(palette.dark ? 0.22 : 0.16, true)), core = makeRibbon(THREE, N + 1, mk(1));
    [env, glow, core].forEach((r) => { scene.add(r.mesh); own.push(r.geo); });
    return { key: b.key, i, env, glow, core, v: 0, c: palette.bands[i] as Rgb };
  });
  let w = 1, h = 1, cx = 0, cy = 0, R0 = 1, sp = 1, hot = -1;
  const radius = (i: number) => R0 * 0.44 + i * sp + sp * 0.3;
  const loop = (i: number, amp: number, t: number): number[][] => {
    const c = rings[i].c, r = radius(i), pts: number[][] = [];
    for (let k = 0; k <= N; k++) {
      const a = (k / N) * Math.PI * 2, rr = r + amp * Math.sin(KF[i] * a + t * (0.8 + i * 0.25) + i * 1.3) * (1 + 0.15 * Math.sin(a * 2 + t));
      pts.push([cx + rr * Math.cos(a), cy + rr * Math.sin(a), c[0], c[1], c[2]]);
    }
    return pts;
  };
  const target = () => live.current;
  return {
    scene, camera,
    update(t, still) {
      const tg = target();
      rings.forEach((rg, i) => {
        const goal = tg ? tg[rg.key] : 0;
        rg.v = still ? goal : rg.v + (goal - rg.v) * 0.15; // reduced motion: no easing, no wobble
        const amp = (rg.v / 40) * sp * 0.62, tt = still ? 0 : t;
        const pts = loop(i, amp, tt);
        rg.core.set(pts, hot === i ? 3 : 2); rg.glow.set(pts, hot === i ? 8 : 5);
        const hi = ranges ? ranges[rg.key].hi : null;
        rg.env.set(loop(i, 0, 0), hi == null ? 0 : (hi / 40) * sp * 0.62 * 2.5);
      });
    },
    resize(nw, nh) {
      w = Math.max(1, nw); h = Math.max(1, nh); camera.left = 0; camera.right = w; camera.top = 0; camera.bottom = h; camera.updateProjectionMatrix();
      cx = w / 2; cy = h / 2; R0 = Math.min(w, h) / 2 - 8; sp = R0 * 0.13;
    },
    pointer(x, y) {
      const prev = hot;
      if (x == null || y == null) hot = -1;
      else { const d = Math.hypot(x - cx, y - cy); let best = -1, bd = sp * 0.6; rings.forEach((_, i) => { const e = Math.abs(radius(i) - d); if (e < bd) { bd = e; best = i; } }); hot = best; }
      io.hover(hot >= 0 && x != null && y != null ? { i: hot, x, y } : null);
      if (prev !== hot) ctx.invalidate();
    },
    dispose() { own.forEach((o) => o.dispose()); scene.clear(); },
  };
}

/**
 * Live Muse bands. `bands` is the live telemetry the Neuro page already has (null when Muse is not streaming);
 * this never opens a second connection. Not streaming: hidden, with one small note.
 */
export function NeuroLiveRing({ bands, ranges, onFail }: { bands: Partial<Record<RingKey, number>> | null; ranges: Record<RingKey, Range> | null; onFail: () => void }) {
  const shares = bandShares(bands);
  const liveRef = useRef<Shares | null>(shares);
  const [hover, setHover] = useState<Hover>(null);
  const [failed, setFailed] = useState(false);
  // The stage reads the ref; the ref follows the props (≤4 Hz from telemetry).
  useEffect(() => { liveRef.current = shares; });
  const sigRanges = JSON.stringify(ranges);
  const on = shares !== null;
  const build = useCallback((ctx: StageCtx) => buildLive(ctx, liveRef, ranges, { hover: setHover }), [ranges]);
  const { canvasRef } = useStage({ enabled: on && !failed, sig: sigRanges, animated: true, build, onFail: () => { setFailed(true); onFail(); } });
  if (!shares) return <div className="d18-live" data-testid="neuro-live-ring" data-on="false"><p className="d18-note">Shows while Muse is connected</p></div>;
  const rows = bandRows(shares, ranges);
  const lead = rows.reduce((a, b) => (b.share > a.share ? b : a));
  const hov = hover ? rows[hover.i] : null;
  return (
    <div className="d18-live" data-testid="neuro-live-ring" data-on="true">
      {!failed && (
        <div className="d18-live__stage">
          <canvas ref={canvasRef} className="d18-live__canvas" aria-hidden="true" />
          <span className="d18-brain__mid" aria-hidden="true" style={{ left: "50%", top: "50%" }}><small>Leading</small><b>{lead.name}</b></span>
          {hov && hover && <span className="d18-brain__tip" aria-hidden="true" style={{ left: hover.x, top: hover.y }}><b>{hov.name}</b> {hov.range}<br />{hov.share.toFixed(1)}% now<small>{hov.usual ? `Usual ${Math.round(hov.usual.lo)} to ${Math.round(hov.usual.hi)}%` : "Usual range needs 3 sessions with band data"}</small></span>}
        </div>
      )}
      <ul className="d18-live__list" aria-label="Live brainwave bands">
        {rows.map((r) => <li key={r.key}><b>{r.name}</b> <span>{Math.round(r.share)}%</span>{r.usual && <small> usual {Math.round(r.usual.lo)} to {Math.round(r.usual.hi)}{r.place && r.place !== "within" ? ` · ${placeWords(r.place)}` : ""}</small>}</li>)}
      </ul>
      <p className="d18-note">Live share of power{ranges ? ", usual as the soft halo" : ""}</p>
    </div>
  );
}

export const liveGlAvailable = glOk;
