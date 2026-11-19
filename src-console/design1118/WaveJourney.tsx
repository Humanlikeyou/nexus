import { Button } from "../ui";
import type { HemisyncTrack, HemisyncWave } from "../neuro/hemisync/adapters";
import { smoothPath } from "./pathModel";
import "./design1118.css";

/** The track he is on: the first track (in wave order) with no Muse-recorded session yet. */
export function journeyPosition(waves: HemisyncWave[], doneIds: Set<string>) {
  const tracks = waves.flatMap((w) => w.tracks);
  const idx = tracks.findIndex((t) => !doneIds.has(t.id));
  const next = idx < 0 ? null : tracks[idx];
  const wave = next ? waves.find((w) => w.wave === next.wave) ?? null : waves[waves.length - 1] ?? null;
  return { total: tracks.length, done: tracks.filter((t) => doneIds.has(t.id)).length, next, wave };
}

/**
 * design-1118a Hemi-Sync (round 5): the collection as a journey. Waves are stations on one wave line,
 * the current wave opens into its tracks, and one next-track card. Start never plays without Muse
 * (enforced by useHemisyncRun.begin); the source folder is only read.
 */
export function WaveJourney({ waves, doneIds, timesDone, connected, pickedId, onPick }: {
  waves: HemisyncWave[]; doneIds: Set<string>; timesDone: (id: string) => number; connected: boolean; pickedId: string | null; onPick: (t: HemisyncTrack) => void;
}) {
  const pos = journeyPosition(waves, doneIds);
  if (!pos.total) return null;
  const W = 680, H = 160, pad = 48;
  const pts = waves.map((w, i) => ({ x: pad + (waves.length === 1 ? 0.5 : i / (waves.length - 1)) * (W - 2 * pad), y: i % 2 ? 56 : 116, w }));
  const curIdx = Math.max(0, waves.findIndex((w) => w.wave === pos.wave?.wave));
  const line = smoothPath(pts);
  const lit = smoothPath(pts.slice(0, curIdx + 1));
  const cur = pos.wave;
  const inWave = cur ? cur.tracks.filter((t) => doneIds.has(t.id)).length : 0;
  const frac = pos.done / pos.total;
  return (
    <div className="d18-card d18-wj" data-testid="wave-journey">
      <div className="d18-wj__head">
        <div className="d18-mring" data-tone="mint" style={{ width: 64, height: 64 }} role="img" aria-label={`${pos.done} of ${pos.total} tracks done`}>
          <svg width="64" height="64" viewBox="0 0 64 64" aria-hidden="true"><circle className="d18-mring__track" cx="32" cy="32" r="27" /><circle className="d18-mring__fill" cx="32" cy="32" r="27" strokeDasharray={`${Math.max(frac * 169.6, 2).toFixed(1)} 169.6`} transform="rotate(-90 32 32)" /></svg>
          <div className="d18-mring__v" aria-hidden="true"><b>{pos.done}/{pos.total}</b><small>done</small></div>
        </div>
        <div><p className="d18-eyebrow d18-eyebrow--quiet">Journey</p><h3 className="d18-h">{cur ? `Wave ${cur.wave} · ${cur.waveName}` : "All waves"}</h3>{cur && <p className="d18-note">{inWave} of {cur.tracks.length} in this wave</p>}</div>
      </div>
      <svg className="d18-wj__map" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Waves: ${waves.map((w) => w.waveName).join(", ")}. You are in ${cur?.waveName ?? "the last wave"}.`}>
        <path className="d18-path__track" d={line} />
        {curIdx > 0 && <path className="d18-path__fill" d={lit} />}
        {pts.map((p, i) => {
          const here = i === curIdx, top = i % 2 === 1;
          const n = p.w.tracks.filter((t) => doneIds.has(t.id)).length;
          // Top stations label above; bottom stations label below. "You are here" sits on the free side.
          const ly = top ? (here ? 30 : -30) : 26, sy = ly + 13; // 11px labels on a 13-unit (Fibonacci) line pitch: they never touch
          return (
            <g key={p.w.wave} className={`d18-st ${here ? "d18-st--current" : n === p.w.tracks.length && n > 0 ? "d18-st--done" : "d18-st--upcoming"}`} transform={`translate(${p.x} ${p.y})`}>
              {here && <circle className="d18-st__halo" r="20" />}
              <circle className="d18-st__dot" r={here ? 11 : 7} />
              {here && <text className="d18-st__here" y="-28" textAnchor="middle">You are here</text>}
              <text className="d18-st__label" y={ly} textAnchor="middle">{p.w.waveName}</text>
              <text className="d18-st__label d18-st__label--sub" y={sy} textAnchor="middle">{n} of {p.w.tracks.length}</text>
            </g>
          );
        })}
      </svg>
      {cur && (
        <>
          <p className="d18-eyebrow d18-eyebrow--quiet">Inside {cur.waveName}</p>
          <ol className="d18-wj__tracks">
            {cur.tracks.map((t) => {
              const d = doneIds.has(t.id), nx = t.id === pos.next?.id;
              return <li key={t.id} data-state={d ? "done" : nx ? "next" : "open"}>{nx && <span className="d18-wj__nexttag">Next</span>}<i aria-hidden="true">{d ? "✓" : ""}</i><b>{t.title}</b>{d && <small>Done {timesDone(t.id)}×</small>}</li>;
            })}
          </ol>
        </>
      )}
      {pos.next ? (
        <section className="d18-next" aria-label="Next track">
          <p className="d18-eyebrow">Next track · Wave {pos.next.wave}</p>
          <h3 className="d18-next__t">{pos.next.title}</h3>
          <p className="d18-next__why">Start records with the Muse and plays the track together, so it never plays on its own.{connected ? "" : " Connect your Muse first."}</p>
          <div className="d18-actions"><Button size="small" variant={pickedId === pos.next.id ? "secondary" : "primary"} onClick={() => onPick(pos.next!)} disabled={pickedId === pos.next.id}>{pickedId === pos.next.id ? "Picked" : "Pick this track"}</Button></div>
        </section>
      ) : <p className="d18-note">Every track has a Muse session. The journey is complete.</p>}
    </div>
  );
}
