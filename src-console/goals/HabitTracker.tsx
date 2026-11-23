import type { DayMark, Habit } from "../personal/adapters";
import "./goals.css";

export const WEEKS = 12;

/** Consecutive "met" days ending today, or null when nothing is known (never a made-up zero). */
export function streakOf(days: DayMark[]): number | null {
  if (!days.some((d) => d !== "none")) return null;
  let n = 0;
  for (let i = days.length - 1; i >= 0; i--) { if (days[i] === "met") n++; else if (i === days.length - 1 && days[i] === "none") continue; else break; }
  return n;
}
/** "Never miss twice": only when yesterday is a known miss and today isn't met yet. */
export const missedYesterday = (days: DayMark[]) => days.length >= 2 && days[days.length - 2] === "missed" && days[days.length - 1] !== "met";
export const hasData = (days: DayMark[]) => days.some((d) => d !== "none");

/** 12 weeks x 7 days. Only the days we know fill the newest cells; everything else stays honestly empty. */
export function Heatmap({ days, label }: { days: DayMark[]; label: string }) {
  const cells: DayMark[] = [...Array<DayMark>(Math.max(0, WEEKS * 7 - days.length)).fill("none"), ...days].slice(-WEEKS * 7);
  const c = (m: DayMark) => cells.filter((x) => x === m).length;
  const aria = hasData(days) ? `${label}, last ${WEEKS} weeks: ${c("met")} met, ${c("nearly")} nearly, ${c("missed")} missed, the rest no data` : `${label}, last ${WEEKS} weeks: no data yet`;
  return <span className="gl-heat" role="img" aria-label={aria}>{cells.map((m, i) => <i key={i} data-m={m} data-today={i === cells.length - 1 || undefined} />)}</span>;
}

/** "Today" strip: each habit's mark for today, counted from its source (no hand ticks). */
export function TodayStrip({ habits }: { habits: Habit[] }) {
  if (!habits.length) return null;
  return (
    <ul className="gl-today" aria-label="Habits today">
      {habits.map((h) => {
        const t = h.days.length ? h.days[h.days.length - 1] : "none";
        const text = t === "met" ? "Done today" : t === "nearly" ? "Nearly" : t === "missed" ? "Not today" : "Waiting for data";
        return <li key={h.id} className="gl-today__item" data-m={t}><span className="gl-today__dot" aria-hidden="true" /><span>{h.title}</span><small>{text}</small></li>;
      })}
    </ul>
  );
}

export function HabitRow({ h }: { h: Habit }) {
  const s = streakOf(h.days);
  const known = hasData(h.days);
  return (
    <div className="gl-habit">
      <div className="gl-habit__head">
        <div><b className="gl-habit__t">{h.title}</b><small className="pk-sub">{h.source} · {h.rule}</small></div>
        <div className="gl-streak">{s === null ? <span className="pk-sub">No streak yet</span> : <><b>{s}</b><small className="pk-sub">day streak</small></>}</div>
      </div>
      <Heatmap days={h.days} label={h.title} />
      {!known && <p className="gl-note">Starts counting once you mark a day.</p>}
      {missedYesterday(h.days) && <p className="gl-note">Missed yesterday. Today keeps it from turning into a habit of missing.</p>}
    </div>
  );
}
