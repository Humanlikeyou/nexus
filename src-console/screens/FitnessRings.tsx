import type { CSSProperties, ReactNode } from "react";
import { Card } from "../ui";
import { DataTag } from "../ui/StatusText";
import { USUAL_MIN, type Driver, type EffortModel, type RecoveryModel, type RingModel, type WeekPoint, type Zone } from "./fitnessRingsModel";
import "./FitnessRings.css";

/** design-1111r option B "Horizon" (Taisun, 13:47 JST), without the Sleep tile: the Sleep card sits next to it. */
const PENDING = "Arrives after the sleep record closes";
const BAND_COLOR = { High: "var(--data-recovery-high)", Medium: "var(--data-recovery-mid)", Low: "var(--data-recovery-low)" } as const;
const ZONE_COLOR: Record<Zone["key"], string> = { Z1: "var(--data-sleep-light)", Z2: "var(--data-recovery-high)", Z3: "var(--data-recovery-mid)", Z4: "var(--copper)", Z5: "var(--data-recovery-low)" };
const r0 = (v: number) => Math.round(v);
const short = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });

/** Small 20-segment ring; the accessible meter for the card. */
function SegRing({ value, max = 100, label, valueText, color }: { value: number | null; max?: number; label: string; valueText?: string; color: string }) {
  const n = 20, C = 27.5, R = 23, on = value == null ? 0 : Math.round((value / max) * n);
  return (
    <div className="rxB__ring" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={max} aria-valuenow={value ?? undefined} aria-valuetext={valueText ?? (value == null ? "No data" : undefined)}>
      <svg viewBox="0 0 55 55" aria-hidden="true">
        {Array.from({ length: n }, (_, i) => {
          const a = ((-90 + (i + .5) * (360 / n)) * Math.PI) / 180;
          return <line key={i} className="rxB__seg" data-on={i < on || undefined} x1={C + (R - 3) * Math.cos(a)} y1={C + (R - 3) * Math.sin(a)} x2={C + (R + 1) * Math.cos(a)} y2={C + (R + 1) * Math.sin(a)} style={i < on ? { stroke: color } : undefined} />;
        })}
      </svg>
    </div>
  );
}

function Scale({ value, usual, ends, grad }: { value: number; usual: number | null; ends: [string, string]; grad: string }) {
  const pct = (v: number) => `${Math.min(100, Math.max(0, v))}%`;
  return (
    <div className="rxB__scale-wrap" aria-hidden="true">
      <div className="rxB__scale" style={{ "--rx-g": grad } as CSSProperties}>
        <span className="t" />
        <span className="v" style={{ width: pct(value) }} />
        {usual != null && <span className="u" style={{ left: pct(usual) }} />}
        <span className="m" style={{ left: pct(value) }} />
      </div>
      <div className="rxB__ends"><span>{ends[0]}</span>{usual != null && <span className="ul" style={{ left: pct(usual) }}>usual</span>}<span>{ends[1]}</span></div>
    </div>
  );
}

function Week({ week, usual, label, unit }: { week: WeekPoint[]; usual: number | null; label: string; unit: string }) {
  const vals = week.map((w) => w.value).filter((v): v is number => v != null);
  if (vals.length < 2) return null;
  const W = 200, H = 34, max = Math.max(...vals, usual ?? 0) * 1.1 || 1, x = (i: number) => (i * W) / (week.length - 1), y = (v: number) => H - (v / max) * H;
  const runs: Array<Array<[number, number]>> = [];
  let cur: Array<[number, number]> = [];
  week.forEach((w, i) => { if (w.value == null) { if (cur.length) runs.push(cur); cur = []; } else cur.push([x(i), y(w.value)]); });
  if (cur.length) runs.push(cur);
  const last = week.length - 1;
  const aria = `${label}, last ${week.length} days: ${week.map((w) => `${short(w.day)} ${w.value == null ? "no record" : `${r0(w.value)}${unit}`}`).join(", ")}${usual != null ? `; usual ${r0(usual)}${unit}` : ""}`;
  return (
    <div className="rx__week">
      <span className="rx__k">Last {week.length} days{usual != null ? " · usual dashed" : ""}</span>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={aria}>
        {runs.map((pts, k) => <g key={k}>
          {pts.length > 1 && <path className="rx__area" d={`M${pts[0][0]},${H} L${pts.map((p) => p.join(",")).join(" L")} L${pts[pts.length - 1][0]},${H} Z`} />}
          <polyline className="rx__line" points={pts.map((p) => p.join(",")).join(" ")} />
        </g>)}
        {usual != null && <line className="rx__usual" x1={0} x2={W} y1={y(usual)} y2={y(usual)} />}
      </svg>
      {/* Dots outside the stretched SVG so they stay round. */}
      <div className="rx__dots" aria-hidden="true">
        {week.map((w, i) => w.value == null ? null : <i key={w.day} data-last={i === last || undefined} style={{ left: `${(i / last) * 100}%`, top: `${(y(w.value) / H) * 100}%` }} />)}
      </div>
    </div>
  );
}

function Versus({ m, unit }: { m: RingModel; unit: string }) {
  if (m.usual == null) return <span className="rx__vs" data-flat>usual after {USUAL_MIN} days ({m.usualCount} of {USUAL_MIN})</span>;
  const d = r0((m.value as number) - m.usual);
  return <span className="rx__vs" data-flat={d === 0 || undefined}>{d === 0 ? "≈" : d > 0 ? "▲" : "▼"} {d === 0 ? "" : `${Math.abs(d)} `}vs usual {r0(m.usual)}{unit}</span>;
}

function DriverTile({ d }: { d: Driver }) {
  const delta = d.delta == null ? null : r0(d.delta);
  return (
    <li>
      <span>{d.label}</span>
      <b>{d.value == null ? "No reading" : r0(d.value)}{d.value != null && <span className="rx__k"> {d.unit}</span>}</b>
      <span className="rx__d" data-flat={delta == null || delta === 0 || undefined}>
        {d.value == null ? "" : delta == null ? `usual after ${USUAL_MIN} days` : delta === 0 ? `≈ usual ${r0(d.usual as number)}` : `${delta > 0 ? "▲" : "▼"} ${Math.abs(delta)} vs ${r0(d.usual as number)}`}
      </span>
    </li>
  );
}

function Unwired({ title, src }: { title: string; src: string }) {
  return (
    <Card title={title} className="fitness__ring-card rx" accessory={<DataTag kind="wire" />} data-state="unwired">
      <div className="rx__body">
        <div className="rxB__top"><p className="rx__num" data-empty>—</p></div>
        <p className="rx__wire"><b>{title}</b> · {src} Nothing here is made up.</p>
        <span className="rx__skel" style={{ width: "89%" }} /><span className="rx__skel" style={{ width: "55%" }} />
      </div>
    </Card>
  );
}

function Pending({ title, m, unit, text, ring }: { title: string; m: RingModel; unit: string; text: string; ring?: ReactNode }) {
  return (
    <Card title={title} className="fitness__ring-card rx" data-state="pending">
      <div className="rx__body">
        <div className="rxB__top"><div className="rxB__lead"><p className="rx__num" data-empty aria-hidden="true">…</p>{m.usual != null && <span className="rx__k">usual {r0(m.usual)}{unit}</span>}</div>{ring}</div>
        <p className="fitness__pending">{text}</p>
        <Week week={m.week} usual={m.usual} label={title} unit={unit} />
      </div>
    </Card>
  );
}

export function RecoveryCard({ m }: { m: RecoveryModel }) {
  if (m.state === "unwired") return <Unwired title="Recovery" src="Appears once NOOP or another band sends a nightly score." />;
  if (m.state === "pending" || m.value == null) return <Pending title="Recovery" m={m} unit="%" text={PENDING} />;
  const color = m.band ? BAND_COLOR[m.band] : "var(--data-recovery-high)";
  return (
    <Card title="Recovery" className="fitness__ring-card rx" data-state={m.state}>
      <div className="rx__body" style={{ "--rx-c": color } as CSSProperties}>
        <div className="rxB__top">
          <div className="rxB__lead"><p className="rx__num">{r0(m.value)}<small>%</small></p><Versus m={m} unit="%" /></div>
          <SegRing value={r0(m.value)} label="Recovery" valueText={`${r0(m.value)}%${m.band ? `, ${m.band}` : ""}`} color={color} />
        </div>
        <Scale value={m.value} usual={m.usual} ends={["Low", "High"]} grad="var(--data-recovery-low), var(--data-recovery-mid) 50%, var(--data-recovery-high)" />
        <ul className="rxB__f" aria-label="What moved recovery">{m.drivers.map((d) => <DriverTile key={d.key} d={d} />)}</ul>
        <Week week={m.week} usual={m.usual} label="Recovery" unit="%" />
        {m.takeaway && <p className="rx__take">{m.takeaway}</p>}
      </div>
    </Card>
  );
}

function Zones({ zones }: { zones: EffortModel["zones"] }) {
  if (zones.state === "unwired") return <p className="rx__wire"><b>Zone minutes</b> <DataTag kind="wire" /> NOOP stores the day&apos;s Effort score only; minutes per heart-rate zone need the HR stream.</p>;
  const z = zones.value, total = z.reduce((s, x) => s + x.minutes, 0);
  return (
    <div className="rx__zone-block">
      <div className="rx__zone-head"><span className="rx__k">Zone minutes</span>{zones.state === "sample" && <DataTag kind="sample" />}</div>
      <div className="rx__zones" role="img" aria-label={z.map((x) => `${x.key} ${x.minutes} minutes`).join(", ")}>
        {z.filter((x) => x.minutes > 0).map((x) => <i key={x.key} style={{ flexGrow: x.minutes, background: ZONE_COLOR[x.key] }} />)}
      </div>
      <ul className="rx__zl" aria-hidden="true">{z.map((x) => <li key={x.key}><i style={{ background: ZONE_COLOR[x.key] }} />{x.key} <b>{x.minutes}m</b></li>)}</ul>
      <span className="rx__k">{total} min with HR above rest</span>
    </div>
  );
}

export function EffortCard({ m, max }: { m: EffortModel; max: number }) {
  if (m.state === "unwired") return <Unwired title="Effort" src="Appears once NOOP sends today's strain." />;
  if (m.state === "pending" || m.value == null) return <Pending title="Effort" m={m} unit="" text="No Effort score for today yet" ring={<SegRing value={null} max={max} label="Effort" color="var(--data-strain)" />} />;
  return (
    <Card title="Effort" className="fitness__ring-card rx" data-state={m.state}>
      <div className="rx__body" style={{ "--rx-c": "var(--data-strain)" } as CSSProperties}>
        <div className="rxB__top">
          <div className="rxB__lead"><p className="rx__num">{r0(m.value)}<small> /{max}</small></p><Versus m={m} unit="" /></div>
          <SegRing value={r0(m.value)} max={max} label="Effort" valueText={`${r0(m.value)} of ${max}`} color="var(--data-strain)" />
        </div>
        <Scale value={(m.value / max) * 100} usual={m.usual == null ? null : (m.usual / max) * 100} ends={["Rest", "All out"]} grad="color-mix(in srgb, var(--copper) 20%, transparent), var(--copper)" />
        <Zones zones={m.zones} />
        <Week week={m.week} usual={m.usual} label="Effort" unit="" />
        {m.takeaway && <p className="rx__take">{m.takeaway}</p>}
      </div>
    </Card>
  );
}
