import { Card } from "../ui";
import { hoursMinutes, type SleepCardModel } from "./sleepModel";
import "./SleepCard.css";

const COLOR = { deep: "var(--data-sleep-deep)", rem: "var(--data-sleep-rem)", light: "var(--data-sleep-light)", awake: "var(--data-sleep-awake)" } as const;
const short = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const stageText = (m: number) => (m >= 60 ? hoursMinutes(m).replace(/ 0(\d)m$/, " $1m") : `${Math.round(m)}m`);

function Duration({ minutes }: { minutes: number }) {
  const r = Math.round(minutes);
  return <p className="sleep-dawn__dur" aria-label={hoursMinutes(r)}>{Math.floor(r / 60)}<small>h</small> {String(r % 60).padStart(2, "0")}<small>m</small></p>;
}

function Versus({ value, usual, unit, digits = 0 }: { value: number; usual: number; unit: "min" | "pts"; digits?: number }) {
  const d = Math.round(value - usual);
  if (unit === "min") return <span className="sleep-dawn__vs">{Math.abs(d) < 1 ? "≈ your usual" : `${d > 0 ? "▲" : "▼"} ${Math.abs(d) < 60 ? `${Math.abs(d)} min` : hoursMinutes(Math.abs(d))} vs usual ${hoursMinutes(usual)}`}</span>;
  return <span className="sleep-dawn__vs">{Math.abs(d) < 1 ? "≈ usual" : `${d > 0 ? "▲" : "▼"} ${Math.abs(d)} vs ${usual.toFixed(digits)}%`}</span>;
}

function WeekBars({ week, usual }: { week: SleepCardModel["week"]; usual: number | null }) {
  const vals = week.map((w) => w.minutes).filter((m): m is number => m != null);
  if (vals.length < 2) return null;
  const W = 210, H = 24, max = Math.max(...vals, usual ?? 0) * 1.08, bw = W / week.length;
  const y = (v: number) => H - (v / max) * H;
  const label = `Last 7 nights: ${week.map((w) => `${short(w.day)} ${w.minutes == null ? "no record" : hoursMinutes(w.minutes)}`).join(", ")}${usual != null ? `; usual ${hoursMinutes(usual)}` : ""}`;
  return (
    <div className="sleep-dawn__week">
      <span className="sleep-dawn__k">Last 7 nights{usual != null ? " · usual dashed" : ""}</span>
      <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={label}>
        {week.map((w, i) => w.minutes == null ? null : <rect key={w.day} x={i * bw + bw * .2} y={y(w.minutes)} width={bw * .6} height={H - y(w.minutes)} rx={2} data-last={i === week.length - 1 || undefined} className="sleep-dawn__bar" />)}
        {usual != null && <line x1={0} x2={W} y1={y(usual)} y2={y(usual)} className="sleep-dawn__usual" />}
      </svg>
    </div>
  );
}

/** design-1110h option B "Dawn": duration and efficiency against his usual, bed → wake, stage totals, 7 nights, one takeaway. */
export function SleepCard({ model, pending }: { model: SleepCardModel | null; pending?: string }) {
  if (!model || model.durationMin == null) return <Card title="Sleep" className="fitness__sleep-card"><p className="fitness__pending">{pending ?? "No sleep record yet"}</p></Card>;
  const total = model.stages.reduce((s, x) => s + x.minutes, 0);
  return (
    <Card title="Sleep" className="fitness__sleep-card sleep-dawn">
      <div className="sleep-dawn__glow" aria-hidden="true" />
      <div className="sleep-dawn__body fitness__sleep-body">
        <div className="sleep-dawn__top">
          <Duration minutes={model.durationMin} />
          {model.usualMin != null ? <Versus value={model.durationMin} usual={model.usualMin} unit="min" /> : <span className="sleep-dawn__k">no usual yet</span>}
        </div>
        {(model.bed || model.wake) && <p className="sleep-dawn__axis"><span>Bed <b>{model.bed ?? "—"}</b></span><span>Wake <b>{model.wake ?? "—"}</b></span></p>}
        {total > 0 && (
          <>
            <div className="sleep-dawn__stages" role="img" aria-label={model.stages.map((s) => `${s.label} ${Math.round(s.minutes)} minutes`).join(", ")}>
              {model.stages.filter((s) => s.minutes > 0).map((s) => <i key={s.key} data-stage={s.key} style={{ flexGrow: s.minutes, background: COLOR[s.key] }} />)}
            </div>
            <ul className="sleep-dawn__legend" aria-hidden="true">
              {model.stages.map((s) => <li key={s.key}><i data-stage={s.key} style={{ background: COLOR[s.key] }} />{s.label} <b>{stageText(s.minutes)}</b></li>)}
            </ul>
          </>
        )}
        {model.efficiencyPct != null && (
          <p className="sleep-dawn__pill fitness__sleep-note">Efficiency <b>{Math.round(model.efficiencyPct)}%</b>{model.usualEffPct != null && <> <Versus value={Math.round(model.efficiencyPct)} usual={Math.round(model.usualEffPct)} unit="pts" /></>}</p>
        )}
        <WeekBars week={model.week} usual={model.usualMin} />
        {model.takeaway && <p className="sleep-dawn__take">{model.takeaway}</p>}
      </div>
    </Card>
  );
}
