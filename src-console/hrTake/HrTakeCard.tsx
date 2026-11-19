import type { ReactNode } from "react";
import { askMore } from "../amaraTake/takeView";
import { TakeEmber } from "../amaraTake/AmaraTakeCard";
import { useNow } from "../shell/useNow";
import { Rich } from "../takes/parts";
import { TakeRating } from "../takes/TakeRating";
import { DataTag } from "../ui";
import type { Wired } from "../lib/sampleAdapters";
import { ageLabel, HR_MIN_COVERED, HR_WINDOW_MIN, tokyoTime, trendOf, type HrTake, type HrTakeStatus, type HrWindow } from "./adapters";
import { useHrTake } from "./useHrTake";
import "../amaraTake/AmaraTakeCard.css";
import "../takes/takes.css";
import "./hrTake.css";

export type HrVariant = "landing" | "inline";
export type HrTakeCardProps = {
  w: Wired<HrTakeStatus> | null; reading: boolean; now: number; variant: HrVariant;
  /** Today's one thing (1111t), landing only. */
  block?: ReactNode;
  onOpenDevices?: () => void;
};

const range = (from: number, to: number) => `${tokyoTime(from).replace(/\s?[AP]M$/, "")}–${tokyoTime(to)}`;
const sourceWord = (w: HrWindow) => (w.source === "whoop" ? "WHOOP" : "Health sync");

function chipsOf(w: HrWindow) {
  const s = w.stats; if (!s) return [];
  const d = s.last - s.first; const t = trendOf(s);
  return [
    { label: "Avg", value: `${s.avg} bpm` },
    { label: "Range", value: `${s.min}–${s.max}` },
    { label: "Trend", value: t === "steady" ? "≈ steady" : `${d > 0 ? "▲" : "▼"} ${Math.abs(d)}` },
  ];
}

/** The 15-minute line the take read, with your usual band shaded. Gaps (no readings) are dashed, never joined. */
export function HrTrace({ w }: { w: HrWindow }) {
  const W = 377, H = 55;
  const vals = w.perMinute.map((m) => m.bpm).filter((b): b is number => b != null);
  const lo = Math.min(...vals, w.usual?.lo ?? Infinity) - 3, hi = Math.max(...vals, w.usual?.hi ?? -Infinity) + 3;
  const n = Math.max(1, w.perMinute.length - 1);
  const x = (i: number) => 5 + (i * (W - 10)) / n;
  const y = (v: number) => 3 + (H - 6) * (1 - (v - lo) / Math.max(1, hi - lo));
  const runs: Array<Array<[number, number]>> = []; let cur: Array<[number, number]> = [];
  w.perMinute.forEach((m, i) => { if (m.bpm == null) { if (cur.length) runs.push(cur); cur = []; } else cur.push([x(i), y(m.bpm)]); });
  if (cur.length) runs.push(cur);
  const lastRun = runs[runs.length - 1];
  const last = lastRun ? lastRun[lastRun.length - 1] : undefined;
  const s = w.stats;
  const label = s ? `Heart rate, last ${HR_WINDOW_MIN} minutes: ${s.first} to ${s.last}, average ${s.avg}${w.usual ? `; your usual ${w.usual.lo}–${w.usual.hi}` : ""}` : `Heart rate, last ${HR_WINDOW_MIN} minutes: no readings`;
  return (
    <figure className="hrt-trace">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={label} preserveAspectRatio="none">
        {w.usual && <rect className="hrt-trace__band" x="0" width={W} y={y(w.usual.hi)} height={Math.max(1, y(w.usual.lo) - y(w.usual.hi))} rx="3" />}
        {runs.map((r) => (
          <g key={`${r[0][0]}`}>
            <path className="hrt-trace__area" d={`M${r[0][0]},${H} L${r.map((p) => p.join(",")).join(" L")} L${r[r.length - 1][0]},${H} Z`} />
            <polyline className="hrt-trace__line" points={r.map((p) => p.join(",")).join(" ")} />
          </g>
        ))}
        {runs.slice(1).map((r, k) => <line key={`g${k}`} className="hrt-trace__gap" x1={runs[k][runs[k].length - 1][0]} x2={r[0][0]} y1={H - 3} y2={H - 3} />)}
        {last && <circle className="hrt-trace__dot" cx={last[0]} cy={last[1]} r="3" />}
      </svg>
      <figcaption className="hrt-trace__ax"><span>{tokyoTime(w.from)}</span><span>{w.usual ? `Shaded: your usual for this time of day, ${w.usual.lo}–${w.usual.hi}` : `Last ${HR_WINDOW_MIN} min`}</span><span>{tokyoTime(w.to)}</span></figcaption>
    </figure>
  );
}

function Head({ when, tag, small }: { when?: ReactNode; tag?: ReactNode; small?: boolean }) {
  return (
    <header className="take__head">
      <TakeEmber small={small} />
      {small ? <h3 className="take__title">Amara&apos;s take</h3> : <h2 className="take__title">Amara&apos;s take</h2>}
      {when && <span className="take__when">{when}</span>}
      {tag}
    </header>
  );
}

function WindowLine({ w }: { w: HrWindow }) {
  return <p className="hrt-win">From <b>{range(w.from, w.to)}</b> · {sourceWord(w)} · {Math.min(HR_WINDOW_MIN, w.coveredMin)} of {HR_WINDOW_MIN} min</p>;
}

function TakeBody({ take, dim, variant }: { take: HrTake; dim: boolean; variant: HrVariant }) {
  return (
    <div className="hrt-body" data-dim={dim || undefined}>
      {variant === "inline" && <HrTrace w={take.window} />}
      <p className="take__body"><Rich text={take.text} /></p>
      {!dim && take.memory && <p className="take__memory"><span className="t-q" aria-hidden="true">↺</span><span><Rich text={take.memory} /></span></p>}
      <ul className="take__chips" aria-label="Numbers Amara used">
        {chipsOf(take.window).map((c) => <li key={c.label} className="take__chip"><span>{c.label}</span>{c.value}</li>)}
      </ul>
      {!dim && <WindowLine w={take.window} />}
    </div>
  );
}

/** Pure: every state of the heart-rate take. Landing = the compact card under the clock; inline = Today's "Your body today". */
export function HrTakeCard({ w, reading, now, variant, block, onOpenDevices }: HrTakeCardProps) {
  const cls = variant === "landing" ? "take take--compact hrt" : "hrt hrt--inline";
  const small = variant === "inline";
  if (w == null) {
    return variant === "inline" ? <section className={cls} aria-label="Amara's take" aria-busy="true" data-state="checking"><Head small={small} /><p className="hrt-quiet">Checking heart rate…</p></section> : null;
  }
  if (w.state === "unwired") {
    return (
      <section className={cls} aria-label="Amara's take" data-state="unwired">
        <Head small={small} tag={<DataTag kind="wire" />} />
        <p className="hrt-quiet">The heart-rate take needs wiring. Amara will read the last {HR_WINDOW_MIN} minutes of heart rate and write a new take when you come back to Home after 30 minutes. Nothing here is written by Amara yet.</p>
      </section>
    );
  }
  const tag = w.state === "sample" ? <DataTag kind="sample" /> : null;
  const s = w.value;
  const prev = s.kind === "take" ? s.take : s.kind === "none_yet" ? null : s.previous;
  const foot = (take: HrTake) => (
    <div className="take__foot"><span className="take__spacer" />
      {w.state !== "sample" && <TakeRating key={take.id} page="home" takeId={take.id} />}
      <button type="button" className="take__ask" onClick={() => askMore({ from: "Home", chips: chipsOf(take.window), question: take.question, sample: w.state === "sample", text: take.text.replace(/\*\*/g, ""), takeId: take.id, page: "home" })}>Ask more ›</button>
    </div>
  );
  if (reading) {
    return (
      <section className={cls} aria-label="Amara's take" aria-busy="true" data-state="regenerating">
        <Head small={small} when={prev ? `as of ${tokyoTime(prev.generatedAt)}` : undefined} tag={tag} />
        <p className="hrt-reading" role="status"><i aria-hidden="true" />Reading your heart rate from {range(now - HR_WINDOW_MIN * 60_000, now)}…</p>
        {prev && <TakeBody take={prev} dim variant={variant} />}
        {block}
      </section>
    );
  }
  if (s.kind === "take") {
    return (
      <section className={cls} aria-label="Amara's take" data-state={now - s.take.generatedAt < 60_000 ? "fresh" : "current"}>
        <Head small={small} when={<time dateTime={new Date(s.take.generatedAt).toISOString()}>{ageLabel(s.take.generatedAt, now)}</time>} tag={tag} />
        <TakeBody take={s.take} dim={false} variant={variant} />
        {block}
        {foot(s.take)}
      </section>
    );
  }
  const kept = prev ? <> The {tokyoTime(prev.generatedAt)} take is kept.</> : null;
  if (s.kind === "not_enough") {
    return (
      <section className={cls} aria-label="Amara's take" data-state="not-enough">
        <Head small={small} when={prev ? `as of ${tokyoTime(prev.generatedAt)}` : undefined} tag={tag} />
        {variant === "inline" && <HrTrace w={s.window} />}
        <p className="hrt-quiet"><b>Not enough heart rate for a new take.</b> Only <b>{s.window.coveredMin} of the last {HR_WINDOW_MIN} minutes</b> have readings. Amara writes once there are {HR_MIN_COVERED} or more.{kept}</p>
        {block}
      </section>
    );
  }
  if (s.kind === "source_down") {
    const since = s.lastSampleAt != null ? tokyoTime(s.lastSampleAt) : null;
    return (
      <section className={cls} aria-label="Amara's take" data-state="source-down">
        <Head small={small} when={prev ? `as of ${tokyoTime(prev.generatedAt)}` : undefined} tag={tag} />
        <p className="hrt-quiet"><b>No recent heart rate.</b> {since ? <>WHOOP hasn&apos;t synced since <b>{since}</b></> : <>WHOOP isn&apos;t syncing</>}, so Amara won&apos;t write a take from old data. She&apos;ll try again the next time you come back to Home.{kept}</p>
        {onOpenDevices && <button type="button" className="hrt-link" onClick={onOpenDevices}>Check WHOOP in Settings › Devices ›</button>}
        {block}
      </section>
    );
  }
  return (
    <section className={cls} aria-label="Amara's take" data-state="none-yet">
      <Head small={small} tag={tag} />
      <p className="hrt-quiet">Amara writes a take from the last {HR_WINDOW_MIN} minutes of heart rate when you open Home.</p>
      {block}
    </section>
  );
}

/** The take with its triggers: refreshes on open, focus and navigation. No button. */
export function HrTake({ variant, block, onOpenDevices, className }: { variant: HrVariant; block?: ReactNode; onOpenDevices?: () => void; className?: string }) {
  const { w, reading } = useHrTake(variant === "landing" ? "open" : "navigate");
  const now = useNow(30_000).getTime();
  const card = <HrTakeCard w={w} reading={reading} now={now} variant={variant} block={block} onOpenDevices={onOpenDevices} />;
  return variant === "landing" ? <div className={className ? `take-slot ${className}` : "take-slot"}>{card}</div> : card;
}
