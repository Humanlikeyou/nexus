import type { NoopDailyMetric } from "../types";
import { ChartHeader, TrendChart } from "../ui/charts/TrendChart";
import { calendarDays, deltaWords, todayJst, usualRange } from "../ui/charts/chartMath";
import { Card } from "../ui";

type Cell = { key: string; title: string; unit: string; digits: number; pick: (d: NoopDailyMetric) => number | null; bars?: boolean; words?: string };
/** design-1110c C11: the four signals of the last 14 days, one header + curve + usual band each. */
const CELLS: Cell[] = [
  { key: "recovery", title: "Recovery", unit: "%", digits: 0, pick: (d) => d.recovery, words: " pts" },
  // A 0 HRV, resting HR or sleep is a missing reading from the band, not a real value.
  { key: "hrv", title: "HRV", unit: "ms", digits: 0, pick: (d) => (d.avgHrv ? d.avgHrv : null) },
  { key: "rhr", title: "Resting heart rate", unit: "bpm", digits: 0, pick: (d) => (d.restingHr ? d.restingHr : null) },
  { key: "sleep", title: "Sleep", unit: "h", digits: 1, pick: (d) => (d.totalSleepMin ? d.totalSleepMin / 60 : null), bars: true },
];
const short = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
const long = (day: string) => new Date(`${day}T12:00:00Z`).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric", timeZone: "UTC" });

export function LastFourteenDays({ days }: { days: NoopDailyMetric[] }) {
  // One slot per calendar day (JST day strings), so a missing day is a gap, not a squeezed neighbour.
  const recent = calendarDays(days, (d) => d.day, 14);
  const cells = CELLS.map((cell) => ({ cell, values: recent.map(({ item }) => { const v = item ? cell.pick(item) : null; return v != null && Number.isFinite(v) ? v : null; }) }))
    .filter(({ values }) => values.filter((v) => v != null).length >= 2);
  if (cells.length === 0) return null;
  const labels = recent.map((d) => short(d.day));
  return (
    <Card title="Last 14 days" className="fitness__fourteen"
      accessory={<p className="chart-legend" aria-hidden="true"><span><i />Daily value</span><span><i data-key="usual" />Your usual</span></p>}>
      <div className="fitness__grid">
        {cells.map(({ cell, values }) => {
          const nums = values.filter((v): v is number => v != null);
          const last = nums[nums.length - 1];
          const usual = usualRange(values);
          const f = (v: number) => v.toFixed(cell.digits);
          const unitText = cell.unit === "%" ? "%" : ` ${cell.unit}`;
          return (
            <div key={cell.key} className="fitness__cell">
              <ChartHeader title={cell.title} value={f(last)} unit={cell.unit}
                delta={usual ? deltaWords(last, usual.median, cell.words ?? unitText, cell.digits) : null}
                usual={usual ? `usual ${f(usual.median)}${unitText}` : "no usual yet"} />
              <TrendChart values={values} labels={labels} lastLabel={recent[recent.length - 1].day === todayJst() ? "Today" : short(recent[recent.length - 1].day)} label={`${cell.title}, last ${recent.length} days: min ${f(Math.min(...nums))}${unitText}, max ${f(Math.max(...nums))}${unitText}, last ${f(last)}${unitText}`}
                unit={cell.unit} format={(v) => (Number.isInteger(v) ? String(v) : v.toFixed(1))} usual={usual} height={124}
                {...(cell.bars ? { kind: "bar" as const, smoothing: 1, domain: [0, 10] as [number, number], ticks: [0, 5, 10] } : {})}
                {...(cell.unit === "%" ? { domain: [0, 100] as [number, number], ticks: [0, 50, 100] } : {})}
                readout={(i) => (
                  <>
                    <strong>{long(recent[i].day)}</strong>
                    <span>{cell.title} <em>{values[i] == null ? "—" : `${f(values[i] as number)}${unitText}`}</em></span>
                    {usual && <span>Usual {f(usual.low)}–{f(usual.high)}{unitText}</span>}
                  </>
                )} />
            </div>
          );
        })}
      </div>
    </Card>
  );
}
