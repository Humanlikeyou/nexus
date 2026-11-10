import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { Button, Card, DataTag, Dialog, PageHero, SegmentedControl, StatusText, wiredTag, type StatusKind } from "../ui";
import type { Wired } from "../lib/sampleAdapters";
import { COLOR, laneFor } from "../neuro/LiveSignal";
import { RETENTION_DAYS, type BandName, type RecordingListItem, type RecordingsAdapter, type SessionSeries, type WindowSecs } from "../neuro/recordings";
import { SignalLanes } from "../neuro/SignalLanes";
import "../neuro/Recordings.css";

/** design-1110e E1–E8, E11, E12: Neuro history as recordings. Data only through RecordingsAdapter. */
const dateFmt = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
const longFmt = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric", hour: "numeric", minute: "2-digit", timeZoneName: "short" });
export const clock = (secs: number) => { const s = Math.max(0, secs); const m = Math.floor(s / 60); const r = s - m * 60; return `${String(m).padStart(2, "0")}:${r.toFixed(r % 1 ? 1 : 0).padStart(r % 1 ? 4 : 2, "0")}`; };
export const length = (secs: number) => { if (secs < 60) return `${secs} s`; const m = Math.floor(secs / 60), s = secs % 60; return s ? `${m} min ${s} s` : `${m} min`; };
const whole = (v: number | null) => (v == null ? "—" : String(Math.round(v)));

export function ChannelBadge({ item }: { item: Pick<RecordingListItem, "channels" | "aux">; short?: boolean }) {
  const five = item.channels.length === 5;
  return <span className="rec-badge" {...(five ? { "data-aux": "" } : {})}><b>{item.channels.length}</b> channels{five ? " · AUX" : ""}</span>;
}
const RAW: Record<RecordingListItem["raw"], [StatusKind, string]> = { available: ["ok", "Raw EEG"], recording: ["wait", "Recording"], "summary-only": ["off", "Summary only"], missing: ["fail", "Raw file missing"] };
function rawStatus(r: RecordingListItem): [StatusKind, string] {
  if (r.retention === "expiring") return ["risk", `Over ${RETENTION_DAYS} days · needs your OK`];
  const [k, t] = RAW[r.raw];
  return r.raw === "available" && (r.hasHeart || r.hasMotion) ? [k, "EEG + heart + motion"] : [k, t];
}

type Filter = "all" | "4" | "5";
export function RecordingsList({ list, onOpen }: { list: Wired<RecordingListItem[]>; onOpen: (id: string) => void }) {
  const [filter, setFilter] = useState<Filter>("all");
  if (list.state === "unwired") return <Card title="Recordings" accessory={wiredTag(list)}><p className="rec-note">Recordings appear here once sessions are saved.</p></Card>;
  const rows = list.value.filter((r) => filter === "all" || String(r.channels.length) === filter);
  return (
    <Card title="Recordings" headingLevel={2} className="rec-card" accessory={<span className="rec-acc"><span className="rec-pills"><SegmentedControl label="Channels" size="small" value={filter} options={[{ value: "all", label: "All" }, { value: "4", label: "4 channels" }, { value: "5", label: "5 channels" }]} onChange={(v) => setFilter(v as Filter)} /></span>{wiredTag(list)}</span>}>
      <table className="rec-table" aria-label="Recordings">
        <thead><tr><th scope="col">Started</th><th scope="col">Length</th><th scope="col">Protocol</th><th scope="col">Channels</th><th scope="col" className="rec-num">Focus</th><th scope="col" className="rec-num">Calm</th><th scope="col">Raw signals</th></tr></thead>
        <tbody>
          {rows.map((r) => {
            const [kind, text] = rawStatus(r);
            return (
              <tr key={r.id} {...(r.retention === "expiring" ? { "data-old": "" } : {})}>
                <td><button type="button" className="rec-open" onClick={() => onOpen(r.id)}>{dateFmt.format(new Date(r.startedAt))}</button></td>
                <td>{r.raw === "recording" ? "Recording now" : length(r.durationSecs)}</td>
                <td>{r.protocol ?? "Muse session"}</td>
                <td><ChannelBadge item={r} /></td>
                <td className="rec-num">{whole(r.avgFocus)}</td>
                <td className="rec-num">{whole(r.avgCalm)}</td>
                <td><StatusText kind={kind}>{text}</StatusText></td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="rec-note">Open a recording to see every channel. 5 channels means the back-of-head (occipital) electrode was detected automatically.</p>
    </Card>
  );
}

/** E12: 90-day retention that never deletes on its own. Removal only through the dialog, only what was ticked. */
export function RetentionCard({ adapter }: { adapter: RecordingsAdapter }) {
  const state = adapter.retention();
  const expiring = state.state === "unwired" ? [] : state.value.expiring;
  const [open, setOpen] = useState(false);
  const [kept, setKept] = useState(false);
  const [ticked, setTicked] = useState<Set<string>>(() => new Set(expiring.map((r) => r.id)));
  const [note, setNote] = useState<string | null>(null);
  const confirm = () => {
    const ids = expiring.filter((r) => ticked.has(r.id)).map((r) => r.id);
    setOpen(false);
    if (!ids.length) return;
    const result = adapter.removeConfirmed(ids, { confirmedByUser: true, at: new Date().toISOString() });
    setNote(result.state === "unwired" ? "Removing recordings needs wiring, so nothing was deleted." : result.state === "sample" ? `Sample data: nothing was deleted (${result.value.removed.length} would be removed).` : `${result.value.removed.length} removed.`);
  };
  return (
    <Card title="Keep raw recordings" headingLevel={2} className="rec-card" accessory={<span className="rec-acc"><span className="rec-select">{RETENTION_DAYS} days</span>{state.state === "sample" ? wiredTag(state) : <DataTag kind="wire" />}</span>}>
      {expiring.length > 0 && !kept && !note && (
        <div className="rec-banner" role="note">
          <StatusText kind="risk"><b>{expiring.length} recording{expiring.length === 1 ? " is" : "s are"} older than {RETENTION_DAYS} days.</b></StatusText>
          <span>They will be removed only after you confirm; nothing is deleted on its own.</span>
          <span className="rec-banner__actions"><Button variant="secondary" size="small" onClick={() => setKept(true)}>Keep them</Button><Button variant="primary" size="small" onClick={() => setOpen(true)}>Review {expiring.length}</Button></span>
        </div>
      )}
      {note && <p className="rec-note" role="status">{note}</p>}
      <p className="rec-note">Recordings older than {RETENTION_DAYS} days get a &ldquo;needs your OK&rdquo; state and wait for you. Saving this setting needs wiring.</p>
      <Dialog open={open} title={`Remove ${expiring.length} recordings older than ${RETENTION_DAYS} days?`} confirmLabel={`Remove ${ticked.size} recording${ticked.size === 1 ? "" : "s"}`} cancelLabel="Keep all" destructive onConfirm={confirm} onCancel={() => setOpen(false)}>
        <p>These raw recordings would be deleted from this Mac. Their session summaries stay in history. This can&apos;t be undone.</p>
        <ul className="rec-check">
          {expiring.map((r) => (
            <li key={r.id}><label><input type="checkbox" checked={ticked.has(r.id)} onChange={(e) => setTicked((prev) => { const next = new Set(prev); if (e.target.checked) next.add(r.id); else next.delete(r.id); return next; })} /> {dateFmt.format(new Date(r.startedAt))} · {length(r.durationSecs)}</label></li>
          ))}
        </ul>
        <p>{state.state === "sample" ? <>{wiredTag(state)} </> : null}Nothing is removed until you press Remove.</p>
      </Dialog>
    </Card>
  );
}

const BAND_ORDER: Array<[BandName, string, string]> = [["delta", "Delta", "1–4 Hz"], ["theta", "Theta", "4–8 Hz"], ["alpha", "Alpha", "8–13 Hz"], ["beta", "Beta", "13–30 Hz"], ["gamma", "Gamma", "30–45 Hz"]];
const linePath = (vals: number[], h: number) => vals.map((v, i) => `${i ? "L" : "M"}${((i / Math.max(1, vals.length - 1)) * 1000).toFixed(1)},${(h - (v / 100) * h).toFixed(1)}`).join("");

/** Stacked band areas as polygon point lists, bottom band first. */
function stackPoints(s: SessionSeries, h: number): Array<[BandName, string]> {
  const n = s.bands.alpha.length; const x = (i: number) => ((i / Math.max(1, n - 1)) * 1000).toFixed(1); const y = (v: number) => (h - (v / 100) * h).toFixed(1);
  const out: Array<[BandName, string]> = []; let lower: number[] = Array.from({ length: n }, () => 0);
  for (const [k] of BAND_ORDER) {
    const upper = lower.map((l, i) => l + s.bands[k][i]);
    out.push([k, upper.map((u, i) => `${x(i)},${y(u)}`).concat(lower.map((l, i) => `${x(i)},${y(l)}`).reverse()).join(" ")]);
    lower = upper;
  }
  return out;
}

function BandChart({ s, at }: { s: SessionSeries; at: number }) {
  const n = s.bands.alpha.length; const h = 110;
  const shapes = stackPoints(s, h).map(([k, pts]) => <polygon key={k} points={pts} style={{ fill: `color-mix(in srgb, var(--band-${k}) 55%, transparent)`, stroke: `var(--band-${k})` }} strokeWidth={0.8} vectorEffect="non-scaling-stroke" />);
  const idx = Math.min(n - 1, Math.round(at * (n - 1)));
  return (
    <>
      <svg className="rec-plot" viewBox={`0 0 1000 ${h}`} preserveAspectRatio="none" height={h} role="img" aria-label="Relative band power over the session">{shapes}<line className="rec-plot__ph" x1={at * 1000} x2={at * 1000} y1={0} y2={h} vectorEffect="non-scaling-stroke" /></svg>
      <div className="rec-legend">{BAND_ORDER.map(([k, name, range]) => <span key={k} style={{ ["--c" as string]: `var(--band-${k})` }}><i aria-hidden="true" />{name} <small>{range}</small> <b>{Math.round(s.bands[k][idx])}%</b></span>)}</div>
    </>
  );
}

function ScoresChart({ s, at }: { s: SessionSeries; at: number }) {
  const h = 80; const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / Math.max(1, xs.length);
  const idx = Math.min(s.focus.length - 1, Math.round(at * (s.focus.length - 1)));
  return (
    <>
      <svg className="rec-plot" viewBox={`0 0 1000 ${h}`} preserveAspectRatio="none" height={h} role="img" aria-label="Focus and calm over the session">
        <line className="rec-plot__g" x1={0} x2={1000} y1={h / 2} y2={h / 2} vectorEffect="non-scaling-stroke" />
        <path d={linePath(s.focus, h)} fill="none" style={{ stroke: "var(--tone-accent-text)" }} strokeWidth={1.6} vectorEffect="non-scaling-stroke" />
        <path d={linePath(s.calm, h)} fill="none" style={{ stroke: "var(--copper-text)" }} strokeWidth={1.6} vectorEffect="non-scaling-stroke" />
        <line className="rec-plot__ph" x1={at * 1000} x2={at * 1000} y1={0} y2={h} vectorEffect="non-scaling-stroke" />
      </svg>
      <div className="rec-legend"><span style={{ ["--c" as string]: "var(--tone-accent-text)" }}><i aria-hidden="true" />Focus · avg <b>{Math.round(mean(s.focus))}</b></span><span style={{ ["--c" as string]: "var(--copper-text)" }}><i aria-hidden="true" />Calm · avg <b>{Math.round(mean(s.calm))}</b></span><span className="rec-legend__end">at playhead · focus <b>{Math.round(s.focus[idx])}</b> · calm <b>{Math.round(s.calm[idx])}</b></span></div>
    </>
  );
}

const SPANS: WindowSecs[] = [2, 5, 10, 30];
const ticksFor = (start: number, span: number) => Array.from({ length: 6 }, (_, i) => clock(start + (i * span) / 5).replace(/\.0$/, ""));

/** E2–E6, E8, E11: one recording, every channel stacked, scrub and zoom with a shared playhead. */
export function RecordingDetail({ item, adapter, onBack }: { item: RecordingListItem; adapter: RecordingsAdapter; onBack: () => void }) {
  const total = Math.max(1, item.durationSecs);
  const [span, setSpan] = useState<WindowSecs>(5);
  const [pos, setPos] = useState(() => Math.min(Math.max(0, total * 0.37), Math.max(0, total - 5)));
  const [playing, setPlaying] = useState(false);
  const start = Math.min(pos, Math.max(0, total - span));
  const win = useMemo(() => adapter.window(item.id, Math.round(start * 4) / 4, span), [adapter, item.id, start, span]);
  const body = useMemo(() => adapter.body(item.id, Math.round(start * 4) / 4, span), [adapter, item.id, start, span]);
  const series = useMemo(() => adapter.series(item.id), [adapter, item.id]);
  const contact = adapter.contact(item.id);
  const at = Math.min(1, (start + span * 0.48) / total);
  useEffect(() => {
    if (!playing) return;
    const t = window.setInterval(() => setPos((p) => { const next = p + 0.25; if (next >= total - span) { setPlaying(false); return total - span; } return next; }), 250);
    return () => window.clearInterval(t);
  }, [playing, total, span]);
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    const i = SPANS.indexOf(span);
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") { e.preventDefault(); setPos((p) => Math.max(0, Math.min(total - span, p + (e.key === "ArrowRight" ? span / 2 : -span / 2)))); }
    else if ((e.key === "+" || e.key === "=") && i > 0) setSpan(SPANS[i - 1]);
    else if (e.key === "-" && i < SPANS.length - 1) setSpan(SPANS[i + 1]);
  };
  const five = item.channels.length === 5;
  const goodCount = contact.state === "unwired" ? null : Object.values(contact.value).filter((c) => c === "good").length;
  const tag = win.state === "unwired" ? null : wiredTag(win);
  return (
    <div className="sessions page-inset rec-page">
      <div className="rec-crumb"><Button variant="plain" size="small" onClick={onBack}>‹ Neuro history</Button><span>› {dateFmt.format(new Date(item.startedAt))}</span></div>
      <PageHero title="Recording" statement={`${length(item.durationSecs)} · ${item.protocol ?? "Muse session"}`}
        detail={<span className="rec-hero-line">{longFmt.format(new Date(item.startedAt))} · Muse · 256 Hz <ChannelBadge item={item} />{five ? <span className="rec-note">AUX detected</span> : null}{wiredTag(adapter.list())}</span>} />
      <ul className="rec-strip" aria-label="Session summary">
        <li className="rec-tile"><span className="rec-tile__k">Avg focus</span><span className="rec-tile__v">{whole(item.avgFocus)}</span><span className="rec-tile__s">of 100</span></li>
        <li className="rec-tile"><span className="rec-tile__k">Avg calm</span><span className="rec-tile__v">{whole(item.avgCalm)}</span><span className="rec-tile__s">{item.calmPct == null ? "" : `calm ${Math.round(item.calmPct)}% of the time`}</span></li>
        <li className="rec-tile"><span className="rec-tile__k">Time in target</span><span className="rec-tile__v">{item.timeInTarget == null ? "—" : `${Math.round(item.timeInTarget)}%`}</span><span className="rec-tile__s" /></li>
        <li className="rec-tile"><span className="rec-tile__k">Contact</span><span className="rec-tile__v">{goodCount == null ? "—" : `${goodCount} of ${item.channels.length} good`}</span><span className="rec-tile__s">{contact.state === "unwired" ? <DataTag kind="wire" /> : five ? "Occipital AUX found automatically" : wiredTag(contact)}</span></li>
      </ul>

      <Card title="Raw EEG · µV" headingLevel={2} className="rec-card" accessory={<span className="rec-acc"><ChannelBadge item={item} />{win.state !== "unwired" && <span className="rec-pills"><SegmentedControl label="Window" size="small" value={String(span)} options={SPANS.map((s) => ({ value: String(s), label: `${s} s` }))} onChange={(v) => setSpan(Number(v) as WindowSecs)} /></span>}{tag}</span>}>
        {win.state === "unwired"
          ? <div className="rec-wire"><span><DataTag kind="wire" /> {item.raw === "summary-only" ? "This session was saved before raw recording existed; it has its summary only." : "Raw EEG for this session is saved on this Mac, but Nexus can't read it back yet."}</span><small>Reading a window of samples per channel needs a read-only command. Until then this page shows the session summary.</small></div>
          : (
            <>
              <div className="rec-keys">
                <SignalLanes lanes={win.value.lanes.map((l) => laneFor(l.channel, l.values, l.contact))} ticks={ticksFor(start, span)} height={five ? 52 : 58} playhead={0.48} caption={`${clock(start)} to ${clock(start + span)}`} />
              </div>
              <input className="rec-scrub" type="range" min={0} max={Math.max(0, total - span)} step={0.25} value={start} aria-label="Position in recording" aria-valuetext={`${clock(start)} of ${clock(total)}. Plus and minus zoom.`} onKeyDown={onKey} onChange={(e) => setPos(Number(e.target.value))} />
              <div className="rec-transport">
                <Button variant="secondary" size="small" onClick={() => setPlaying((p) => !p)}>{playing ? "Pause" : "Play"}</Button>
                <span><b>{clock(start + span * 0.48)}</b> / {clock(total)}</span>
                <span>← → step · + − zoom · drag to scrub</span>
                <span className="rec-transport__end">256 Hz · up to 900 points per lane</span>
              </div>
            </>
          )}
      </Card>

      <details className="rec-fold ui-card" {...(five ? { open: true } : {})}>
        <summary><span className="rec-fold__title">Heart and motion</span><span className="rec-acc">PPG 64 Hz · motion 52 Hz {body.state === "unwired" ? <DataTag kind="wire" /> : wiredTag(body)}</span></summary>
        {body.state === "unwired" || (!body.value.heart && !body.value.motion)
          ? <div className="rec-wire"><span>Heart (PPG) and motion aren&apos;t recorded yet.</span><small>The headset streams them live; saving them with the recording needs wiring.</small></div>
          : (
            <>
              <SignalLanes scaleLabel={null} lanes={[
                ...(body.value.heart ? [{ key: "heart", name: "Heart", label: "Heart, PPG", where: "PPG · forehead", note: body.value.heart.bpm == null ? undefined : `${body.value.heart.bpm} bpm (estimate)`, color: "var(--landing-heart)", values: body.value.heart.values }] : []),
                ...(body.value.motion ? [{ key: "motion", name: "Motion", label: "Motion, accelerometer and gyro", where: "Accel + gyro", note: body.value.motion.events[0] ? `${body.value.motion.events[0].label} ${clock(body.value.motion.events[0].atSecs)}` : "Still", color: "var(--label-secondary)", values: body.value.motion.values }] : []),
              ]} ticks={ticksFor(start, span)} height={58} playhead={0.48} caption={`${clock(start)} to ${clock(start + span)}`} />
              <p className="rec-note">Same window and playhead as the EEG. Motion explains spikes in the EEG lanes; heart rate is an estimate, not a medical reading.</p>
            </>
          )}
      </details>

      <Card title="Band power · whole session" headingLevel={2} className="rec-card" accessory={<span className="rec-acc">Relative %, 1-second windows {series.state === "unwired" ? <DataTag kind="wire" /> : wiredTag(series)}</span>}>
        {series.state === "unwired" ? <div className="rec-wire"><span>Band power over time isn&apos;t stored. It will be computed from the saved raw EEG when a recording opens.</span></div> : <BandChart s={series.value} at={at} />}
      </Card>
      <Card title="Focus and calm" headingLevel={2} className="rec-card" accessory={<span className="rec-acc">0–100 · scored about twice a second {series.state === "unwired" ? <DataTag kind="wire" /> : wiredTag(series)}</span>}>
        {series.state === "unwired" ? <div className="rec-wire"><span>The focus and calm timeline is saved with the session, but Nexus doesn&apos;t read it back yet.</span></div> : <ScoresChart s={series.value} at={at} />}
      </Card>
      <p className="rec-note">Same ±100 µV scale on every lane. Not a medical measurement.</p>
    </div>
  );
}
export { COLOR };
