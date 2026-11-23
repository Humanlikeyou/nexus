import { Disc3, Download, Play, Square } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { valueOf, type Wired } from "../lib/sampleAdapters";
import type { HemisyncAdapter, HemisyncSession, HemisyncWave, HemisyncTrack, HemisyncTrackStats, ImportProgress } from "../neuro/hemisync/adapters";
import { hemisyncAdapter } from "../neuro/hemisync/live";
import { Button, DataTag, Dialog, InsetGroup, ListRow, More, SegmentedControl, wiredTag } from "../ui";
import { ChartHeader, TrendChart } from "../ui/charts/TrendChart";
import { deltaWords, usualRange } from "../ui/charts/chartMath";
import { useHemisyncRun, notice } from "./useHemisyncRun";
import { hemisyncRanking, type HemiTrackRanking } from "../bridge";
import { WaveJourney, journeyPosition, trackDoneIds } from "../design1118/WaveJourney";
import "../ui/PanelKit.css";
import "./NeuroSections.css";

const day = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", timeZone: "Asia/Tokyo" });
const whole = (v: number | null | undefined) => (v == null || !Number.isFinite(v) ? "—" : String(Math.round(v)));
const minutes = (ms: number | null) => (ms == null ? null : `${Math.max(1, Math.round(ms / 60_000))} min`);
const clock = (secs: number) => `${Math.floor(secs / 60)}:${String(Math.floor(secs % 60)).padStart(2, "0")}`;
const RATINGS = [1, 2, 3, 4, 5] as const;

export type HemisyncSectionProps = {
  adapter?: HemisyncAdapter;
  connected: boolean;
  recording: boolean;
  sessionId: string | null;
  operation: string | null;
  protocolId: string;
  setProtocolId: (id: string) => void;
  onStartMuse: () => void;
  onStopMuse: () => void;
  /** Search for a Muse (the "Connect your Muse first" note offers it). */
  onFindMuse?: () => void;
  /** Opens a session's Muse recording in Neuro history. */
  onOpenRecording?: (sessionId: string) => void;
  /** Overrides the ranking read (tests). */
  ranking?: () => Promise<HemiTrackRanking>;
};

/** One tag per Wired value, shown only when that value's state says so (sample or unwired). */
const tagOf = <T,>(w: Wired<T>) => wiredTag(w);

function ImportBar({ adapter, empty, onDone }: { adapter: HemisyncAdapter; empty: boolean; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [wire, setWire] = useState(false);
  useEffect(() => {
    if (!busy) return;
    const read = () => setProgress(adapter.importProgress());
    read();
    const timer = window.setInterval(read, 500);
    return () => window.clearInterval(timer);
  }, [busy, adapter]);
  const run = async () => {
    setBusy(true); setResult(null); setWire(false);
    try {
      const out = await adapter.importFrom();
      if (out.state === "unwired") setWire(true);
      else setResult(`${out.value.added} added, ${out.value.skipped} already there.`);
    } catch {
      setResult("The import didn't finish. Your Desktop folder wasn't changed. Try again in a moment.");
    } finally { setBusy(false); setProgress(null); onDone(); }
  };
  const pct = progress && progress.total > 0 ? Math.round((progress.copied / progress.total) * 100) : 0;
  return (
    <div className="hs-import">
      <Button variant={empty ? "primary" : "secondary"} size="small" icon={Download} onClick={() => { void run(); }} loading={busy} disabled={busy}>{busy ? "Importing…" : empty ? "Import your collection" : "Import again"}</Button>
      {busy && (
        <div className="hs-import__bar" role="progressbar" aria-label="Import progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progress ? pct : undefined} aria-valuetext={progress ? `${progress.copied} of ${progress.total} tracks, ${progress.phase}` : "Starting"}>
          <i style={{ transform: `scaleX(${pct / 100})` }} />
        </div>
      )}
      {busy && <p className="hs-note" role="status">{progress ? `${progress.copied} of ${progress.total} · ${progress.phase}` : "Looking for your folder…"}</p>}
      {result && <p className="hs-note" role="status">{result}</p>}
      {wire && <p className="hs-note" role="status"><DataTag kind="wire" /> Importing isn't connected yet.</p>}
    </div>
  );
}

function RatingSheet({ title, onSave, onSkip }: { title: string; onSave: (rating: 1 | 2 | 3 | 4 | 5, note: string) => void; onSkip: () => void }) {
  const [rating, setRating] = useState<1 | 2 | 3 | 4 | 5 | null>(null);
  const [note, setNote] = useState("");
  const [tried, setTried] = useState(false);
  return (
    <Dialog open title="How was that session?" confirmLabel="Save" cancelLabel="Skip" onConfirm={() => { if (rating) onSave(rating, note); else setTried(true); }} onCancel={onSkip} className="hs-sheet">
      <p className="hs-note">{title}</p>
      <SegmentedControl label="Rating, 1 to 5" value={rating ? String(rating) : ""} options={RATINGS.map((n) => ({ value: String(n), label: String(n) }))} onChange={(v) => setRating(Number(v) as 1 | 2 | 3 | 4 | 5)} />
      <p className="hs-note" {...(tried && rating == null ? { role: "alert", "data-tone": "warning" } : {})}>1 is rough, 5 is very good. {rating == null ? (tried ? "Nothing was saved. Pick a number from 1 to 5, then press Save, or press Skip." : "Pick a number to save.") : ""}</p>
      <label className="hs-field">Note (optional)
        <textarea rows={2} value={note} onChange={(e) => setNote(e.target.value)} placeholder="What you noticed" />
      </label>
    </Dialog>
  );
}

const mean = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);
/** "+6 vs usual", only when there is a real usual to compare with. */
const vsUsual = (v: number | null, usual: number | null) => {
  if (v == null || usual == null) return "";
  const d = Math.round(v) - Math.round(usual);
  return d === 0 ? " · at your usual" : ` · ${d > 0 ? "+" : "−"}${Math.abs(d)} vs usual`;
};

/**
 * "What the Muse found": what the recorded Hemi-Sync sessions say, not a checklist (the Wave journey above already
 * counts tracks). Focus and calm per track against his usual (the mean of all his Hemi-Sync sessions), the best track
 * for each, sessions still unrated, and the next track to try. Every number is a plain mean of his own sessions.
 */
export function DoneWithMuse({ tracks, waves, stats, sessions, onRate, onOpenRecording }: { tracks: HemisyncTrack[]; waves: HemisyncWave[]; stats: Wired<HemisyncTrackStats[]>; sessions: Wired<HemisyncSession[]>; onRate?: (sessionId: string, title: string) => void; onOpenRecording?: (sessionId: string) => void }) {
  const rows = valueOf(stats) ?? [];
  const log = [...(valueOf(sessions) ?? [])].sort((a, b) => a.startedAt.localeCompare(b.startedAt));
  const byId = new Map(tracks.map((t) => [t.id, t]));
  const done = rows.filter((r) => r.sessions > 0);
  const doneIds = trackDoneIds(waves, rows); // sessions OR marked done (0045)
  const next = journeyPosition(waves, doneIds).next;
  const labels = log.map((s) => day.format(new Date(s.startedAt)));
  const focusVals = log.map((s) => s.metrics.focusAvg), calmVals = log.map((s) => s.metrics.calmAvg);
  const realCount = (vals: Array<number | null>) => vals.filter((v) => v != null).length;
  // His usual needs at least two scored sessions; one session is only compared with itself.
  const usualFocus = realCount(focusVals) >= 2 ? mean(focusVals.filter((v): v is number => v != null)) : null;
  const usualCalm = realCount(calmVals) >= 2 ? mean(calmVals.filter((v): v is number => v != null)) : null;
  const titleOf = (id: string) => byId.get(id)?.title ?? "Unknown track";
  const best = (pick: (r: HemisyncTrackStats) => number | null) => {
    const scored = done.filter((r) => pick(r) != null);
    if (scored.length < 2) return null; // "best" of one track is not a finding
    return scored.reduce((a, b) => ((pick(b) as number) > (pick(a) as number) ? b : a));
  };
  const bestFocus = best((r) => r.focusAvg), bestCalm = best((r) => r.calmAvg);
  const unrated = done.flatMap((r) => { const u = [...log].reverse().find((x) => x.trackId === r.trackId && x.rating == null); return u ? [u] : []; });
  const chart = (title: string, values: Array<number | null>) => {
    const real = values.filter((v): v is number => v != null);
    const usual = usualRange(values);
    const last = real.length ? real[real.length - 1] : null;
    return (
      <div className="mb-row mb-row--stack">
        <div className="mb-row__head"><ChartHeader title={title} value={whole(last)} unit="/ 100" delta={deltaWords(last, usual?.median ?? null, " pts")} usual={usual ? `usual ${whole(usual.median)}` : "no usual yet"} /></div>
        <div className="mb-plot"><TrendChart values={values} labels={labels} lastLabel={labels[labels.length - 1]} label={`${title} across your Hemi-Sync sessions, latest ${whole(last)} of 100`} domain={[0, 100]} height={120} usual={usual} /></div>
      </div>
    );
  };
  const nextLine = next
    ? `Next track to try: ${next.title} (Wave ${next.wave} · ${next.waveName}).`
    : tracks.length > 0 ? (tracks.some((t) => t.markedDone) ? "Every track is done, some marked done by you." : "You have recorded a session on every track.") : null;
  return (
    <section className="mb-section" aria-labelledby="hs-done">
      <div className="mb-section__head"><h2 id="hs-done" className="mb-section__title">What the Muse found</h2>{tagOf(stats)}</div>
      {stats.state === "unwired"
        ? <p className="mind-body__hint">Your history per track needs wiring before it can show here.</p>
        : done.length === 0
          ? <p className="mind-body__hint">No Hemi-Sync sessions with the Muse yet. {next ? `Pick ${next.title} above and press Start with your Muse on.` : "Pick a track above and press Start with your Muse on."}</p>
          : (
              <>
                <p className="mb-section__lede">{log.length} Hemi-Sync session{log.length === 1 ? "" : "s"} on {done.length} track{done.length === 1 ? "" : "s"}. {usualFocus == null && usualCalm == null ? "Your usual appears after two scored sessions." : `Your usual: focus ${whole(usualFocus)}, calm ${whole(usualCalm)}.`}</p>
                {(bestFocus || bestCalm) && (
                  <ul className="hs-found" aria-label="Best tracks so far">
                    {bestFocus && <li>Best for focus: <strong>{titleOf(bestFocus.trackId)}</strong> · {whole(bestFocus.focusAvg)} / 100</li>}
                    {bestCalm && <li>Best for calm: <strong>{titleOf(bestCalm.trackId)}</strong> · {whole(bestCalm.calmAvg)} / 100</li>}
                  </ul>
                )}
                <table className="mb-table hs-table" aria-label="Focus and calm by track">
                  <thead><tr><th scope="col">Track</th><th scope="col">Sessions</th><th scope="col">Focus</th><th scope="col">Calm</th>{onOpenRecording && <th scope="col">Recording</th>}</tr></thead>
                  <tbody>
                    {done.map((r) => {
                      const t = byId.get(r.trackId);
                      const latest = [...log].reverse().find((x) => x.trackId === r.trackId) ?? null;
                      return (
                        <tr key={r.trackId}>
                          <th scope="row">{t?.title ?? "Unknown track"}<small>{t ? `Wave ${t.wave} · ${t.waveName}` : ""}{r.lastAt ? ` · last ${day.format(new Date(r.lastAt))}` : ""}</small></th>
                          <td>{r.sessions}</td>
                          <td>{whole(r.focusAvg)}<small>{vsUsual(r.focusAvg, usualFocus)}</small></td>
                          <td>{whole(r.calmAvg)}<small>{vsUsual(r.calmAvg, usualCalm)}</small></td>
                          {onOpenRecording && <td>{latest ? <Button variant="plain" size="small" aria-label={`Open the recording of ${t?.title ?? "this track"}`} onClick={() => onOpenRecording(latest.id)}>Open ›</Button> : "—"}</td>}
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
                {unrated.length > 0 && onRate && (
                  <ul className="hs-found" aria-label="Sessions not rated yet">
                    {unrated.map((u) => <li key={u.id}>{titleOf(u.trackId)} · {day.format(new Date(u.startedAt))} not rated <Button variant="plain" size="small" aria-label={`Rate ${titleOf(u.trackId)}`} onClick={() => onRate(u.id, byId.get(u.trackId)?.title ?? "This session")}>Rate</Button></li>)}
                  </ul>
                )}
                {realCount(focusVals) >= 2 && realCount(calmVals) >= 2 && <div className="hs-charts">{chart("Focus", focusVals)}{chart("Calm", calmVals)}</div>}
                {nextLine && <p className="hs-note">{nextLine}</p>}
              </>
            )}
    </section>
  );
}

const MEASURES: Array<[keyof Pick<HemiTrackRanking, "focus" | "calm" | "rating">, string, string]> = [["focus", "Focus", "/ 100"], ["calm", "Calm", "/ 100"], ["rating", "Rating", "/ 5"]];

/** The backend's own ranking (hemisync_ranking, read-only): the mean per track, only for tracks with enough sessions. */
export function BestTracks({ ranking, tracks }: { ranking: HemiTrackRanking | null; tracks: HemisyncTrack[] }) {
  if (!ranking) return null;
  const byId = new Map(tracks.map((t) => [t.id, t]));
  const any = MEASURES.some(([k]) => ranking[k].ranked.length > 0);
  return (
    <section className="mb-section" aria-labelledby="hs-best">
      <div className="mb-section__head"><h2 id="hs-best" className="mb-section__title">Best tracks for you</h2></div>
      {!any ? <p className="mind-body__hint">Once a track has {ranking.minSessions} sessions with the Muse, your best tracks show here.</p> : (
        <div className="hs-charts">
          {MEASURES.map(([key, label, unit]) => ranking[key].ranked.length > 0 && (
            <div key={key} className="mb-row mb-row--stack">
              <h3>{label}</h3>
              <ol aria-label={`Best tracks by ${label.toLowerCase()}`}>
                {ranking[key].ranked.slice(0, 3).map((r) => <li key={r.trackId}>{byId.get(r.trackId)?.title ?? "Unknown track"} · {key === "rating" ? r.mean.toFixed(1) : Math.round(r.mean)} {unit} <small>over {r.n} sessions</small></li>)}
              </ol>
            </div>
          ))}
        </div>
      )}
      <p className="mind-body__hint">Plain averages over your own sessions, not a prediction. Tracks need {ranking.minSessions} sessions to be ranked.</p>
    </section>
  );
}

export function HemisyncSection({ adapter: given, connected, recording, sessionId, operation, protocolId, setProtocolId, onStartMuse, onStopMuse, onFindMuse, onOpenRecording, ranking: readRanking = hemisyncRanking }: HemisyncSectionProps) {
  const adapter = useMemo(() => given ?? hemisyncAdapter(), [given]);
  const [tick, setTick] = useState(0);
  const changed = useCallback(() => setTick((n) => n + 1), []);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `tick` re-reads the adapter after a write
  const lib = useMemo(() => adapter.library(), [adapter, tick]);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const stats = useMemo(() => adapter.stats(), [adapter, tick]);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const sessions = useMemo(() => adapter.sessions(), [adapter, tick]);
  const [pickedId, setPickedId] = useState<string | null>(null);
  const waves = useMemo(() => (lib.state === "unwired" ? [] : lib.value.waves), [lib]);
  const tracks = useMemo(() => waves.flatMap((w) => w.tracks), [waves]);
  // The wave of his most recent Hemi-Sync session opens; until a session exists, the first wave.
  const recentWave = useMemo(() => {
    const latest = [...(valueOf(sessions) ?? [])].sort((a, b) => b.startedAt.localeCompare(a.startedAt))[0];
    return latest ? tracks.find((t) => t.id === latest.trackId)?.wave ?? null : null;
  }, [sessions, tracks]);
  const picked = tracks.find((t) => t.id === pickedId) ?? null;
  const statsById = new Map((valueOf(stats) ?? []).map((s) => [s.trackId, s]));
  const run = useHemisyncRun({ adapter, connected, recording, sessionId, operation, protocolId, setProtocolId, startMuse: onStartMuse, stopMuse: onStopMuse, changed });
  const playing = run.phase !== "idle";
  const empty = lib.state !== "unwired" && tracks.length === 0;
  const [hint, setHint] = useState<string | null>(null);
  const start = (track: HemisyncTrack | null = picked) => {
    if (!track) { setHint("Pick a track first."); return; }
    setPickedId(track.id);
    setHint(null);
    void run.begin(track);
  };
  // Best tracks (hemisync_ranking): read again whenever a session is rated or finished.
  const [ranking, setRanking] = useState<HemiTrackRanking | null>(null);
  const sessionCount = valueOf(sessions)?.length ?? 0;
  const ratedCount = (valueOf(sessions) ?? []).filter((s) => s.rating != null).length;
  useEffect(() => {
    let live = true;
    readRanking().then((r) => { if (live) setRanking(r); }, () => { if (live) setRanking(null); });
    return () => { live = false; };
  }, [readRanking, sessionCount, ratedCount]);
  // "Rate" on a Done-with-Muse row: the same sheet, for a session that was skipped.
  const [late, setLate] = useState<{ sessionId: string; title: string } | null>(null);
  const rateLate = async (rating: 1 | 2 | 3 | 4 | 5, text: string) => {
    if (!late) return;
    const result = await adapter.rate({ sessionId: late.sessionId, rating, ...(text.trim() ? { note: text.trim() } : {}) }).catch(() => null);
    notice(result && result.state !== "unwired" ? `Rated ${late.title} ${rating} of 5` : "The rating couldn't be saved yet");
    changed();
    setLate(null);
  };
  // 0045: hand marks. A mark is its own flag, never a session; every view reads sessions OR the mark.
  const markTrack = async (t: HemisyncTrack, done: boolean) => {
    const r = await adapter.markDone([t.id], done).catch(() => null);
    notice(r && r.state !== "unwired" ? (done ? `Marked ${t.title} done` : `Undid marked done for ${t.title}`) : "The mark couldn't be saved yet");
    changed();
  };
  const markWave = async (wave: number, name: string, done: boolean) => {
    const r = await adapter.markWaveDone(wave, done).catch(() => null);
    notice(r && r.state !== "unwired" ? (done ? `Marked Wave ${wave} ${name} done` : `Undid marked done for Wave ${wave} ${name}`) : "The mark couldn't be saved yet");
    changed();
  };
  const note = hint ?? run.message;

  return (
    <>
      <section className="mb-section" aria-labelledby="hs-title">
        <div className="mb-section__head"><h2 id="hs-title" className="mb-section__title">Hemi-Sync</h2>{tagOf(lib)}</div>
        {lib.state === "unwired" ? (
          <p className="mb-section__lede">Your Hemi-Sync library isn&apos;t connected yet.</p>
        ) : empty ? (
          <p className="mb-section__lede">No tracks yet. Import your collection and it stays on this Mac.</p>
        ) : (
          <p className="mb-section__lede">{tracks.length} track{tracks.length === 1 ? "" : "s"} across {waves.length} wave{waves.length === 1 ? "" : "s"}. Pick one, press Start, and the Muse records while it plays.</p>
        )}
        <ImportBar adapter={adapter} empty={empty || lib.state === "unwired"} onDone={changed} />
        {lib.state !== "unwired" && lib.value.sourceFolder && <p className="hs-note">Copied from {lib.value.sourceFolder}. That folder is only read, never changed.</p>}

        {tracks.length > 0 && (
          <>
            <WaveJourney waves={waves} doneIds={trackDoneIds(waves, valueOf(stats) ?? [])} timesDone={(id) => statsById.get(id)?.sessions ?? 0}
              connected={connected} pickedId={pickedId} onPick={(t) => { if (!playing) { setPickedId(t.id); setHint(null); } }} onStart={(t) => start(t)} playing={playing} />
            <More label="All tracks by wave">
            <div className="hs-waves">
              {waves.map((w, i) => (
                <More key={w.wave} label={`Wave ${w.wave} · ${w.waveName} · ${w.tracks.length} track${w.tracks.length === 1 ? "" : "s"}`} open={(recentWave == null ? i === 0 : w.wave === recentWave) || w.tracks.some((t) => t.id === pickedId) || undefined}>
                  {w.wave > 0 && (() => {
                    const undone = w.tracks.filter((t) => !t.markedDone && !((statsById.get(t.id)?.sessions ?? 0) > 0));
                    const marked = w.tracks.some((t) => t.markedDone);
                    if (undone.length === 0 && !marked) return null;
                    return undone.length > 0
                      ? <Button variant="secondary" size="small" disabled={playing} onClick={() => { void markWave(w.wave, w.waveName, true); }}>Mark wave done</Button>
                      : <Button variant="secondary" size="small" disabled={playing} onClick={() => { void markWave(w.wave, w.waveName, false); }}>Undo wave done</Button>;
                  })()}
                  <InsetGroup className="hs-group" aria-label={`Wave ${w.wave} ${w.waveName} tracks`}>
                    {w.tracks.map((t) => {
                      const s = statsById.get(t.id);
                      const meta = [minutes(t.durationMs), s && s.sessions > 0 ? `Done ${s.sessions}×` : t.markedDone ? "Marked done" : null].filter(Boolean).join(" · ");
                      return <ListRow key={t.id} icon={Disc3} title={t.title} subtitle={meta || undefined} selected={t.id === pickedId} onSelect={() => { if (!playing) setPickedId(t.id); }} disabled={playing && t.id !== pickedId}
                        accessory={t.markedDone
                          ? <Button variant="plain" size="small" aria-label={`Undo marked done for ${t.title}`} onClick={() => { void markTrack(t, false); }}>Undo</Button>
                          : !(s && s.sessions > 0) ? <Button variant="plain" size="small" aria-label={`Mark ${t.title} done`} onClick={() => { void markTrack(t, true); }}>Mark done</Button> : undefined} />;
                    })}
                  </InsetGroup>
                </More>
              ))}
            </div>
            </More>

            <div className="hs-start">
              <p className="hs-start__track" aria-live="polite">{picked ? <><strong>{picked.title}</strong><span>Wave {picked.wave} · {picked.waveName}{minutes(picked.durationMs) ? ` · ${minutes(picked.durationMs)}` : ""}</span></> : <span>No track picked yet</span>}</p>
              {playing ? (
                <Button variant="destructive" icon={Square} onClick={run.stop}>Stop</Button>
              ) : (
                <Button variant="primary" icon={Play} onClick={() => start()} aria-disabled={!connected || undefined} loading={operation === "starting"}>Start</Button>
              )}
            </div>
            {playing && (
              <div className="hs-playing" role="status">
                <span>{run.phase === "starting" ? "Starting the Muse and the track…" : `Playing and recording · ${clock(run.position.at)}${run.position.of ? ` of ${clock(run.position.of)}` : ""}`}</span>
                <div className="hs-import__bar" role="progressbar" aria-label="Track position" aria-valuemin={0} aria-valuemax={100} aria-valuenow={run.position.of ? Math.round((run.position.at / run.position.of) * 100) : 0}><i style={{ transform: `scaleX(${run.position.of ? run.position.at / run.position.of : 0})` }} /></div>
              </div>
            )}
            {!playing && !connected && (
              <div className="hs-note" role="status">
                <p>Connect your Muse first. Start records with the Muse and plays the track together, so it never plays on its own.</p>
                {onFindMuse && <Button variant="secondary" size="small" onClick={onFindMuse} disabled={operation !== null} loading={operation === "scanning"}>Find Muse</Button>}
              </div>
            )}
            {!playing && connected && protocolId !== "off" && <p className="hs-note">Hemi-Sync sessions record only, with no feedback sounds on top of the track.</p>}
            {note && <p className="hs-note" role="status" data-tone="warning">{note}</p>}
            {run.unsaved && <p className="hs-note" role="status"><DataTag kind="wire" /> This session was recorded, but its Hemi-Sync link isn&apos;t saved yet.</p>}
          </>
        )}
      </section>

      <DoneWithMuse tracks={tracks} waves={waves} stats={stats} sessions={sessions} onRate={(sessionId, title) => setLate({ sessionId, title })} onOpenRecording={onOpenRecording} />
      <BestTracks ranking={ranking} tracks={tracks} />

      {late && !run.finished && <RatingSheet key={late.sessionId} title={`${late.title}. Rate how it went.`} onSave={(r, n) => { void rateLate(r, n); }} onSkip={() => setLate(null)} />}
      {run.finished && <RatingSheet title={`${run.finished.title}. Rate how it went.`} onSave={(r, n) => { void run.rate(r, n); }} onSkip={run.dismissFinished} />}
    </>
  );
}
