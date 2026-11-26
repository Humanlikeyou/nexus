import { useState } from "react";
import { TrendChart } from "../ui/charts/TrendChart";
import { CATEGORY_LABEL, formatMajor, type WireCurrencyView } from "./entries";
import { balanceOverTime, compact, categoryCompare, incomeVsExpenses, savingsOverTime } from "./chartData";
import "./moneyCharts.css";

const enough = (xs: readonly (number | null)[]) => xs.filter((x) => x != null).length >= 2;
const Empty = ({ children }: { children: string }) => <p className="mc-empty">{children}</p>;

/** Hidden table that carries the same numbers as a chart, for screen readers. */
function DataTable({ caption, head, rows }: { caption: string; head: string[]; rows: string[][] }) {
  return (
    <table className="ui-visually-hidden">
      <caption>{caption}</caption>
      <thead><tr>{head.map((h) => <th key={h} scope="col">{h}</th>)}</tr></thead>
      <tbody>{rows.map((r) => <tr key={r[0]}>{r.map((c, i) => (i === 0 ? <th key={i} scope="row">{c}</th> : <td key={i}>{c}</td>))}</tr>)}</tbody>
    </table>
  );
}

/** Charts for one currency. JPY and USD are never drawn together or summed. */
export function MoneyCharts({ cv }: { cv: WireCurrencyView }) {
  const c = cv.currency;
  const fmt = compact;
  const [hover, setHover] = useState<number | null>(null);
  const ie = incomeVsExpenses(cv, 6);
  const cat = categoryCompare(cv);
  const sav = savingsOverTime(cv);
  const bal = balanceOverTime(cv);
  const show = (v: number | null) => (v == null ? "–" : `${formatMajor(v, c)} ${c}`);
  const max = Math.max(1, ...cat.rows.flatMap((r) => [r.thisMonth, r.usual ?? 0]));

  return (
    <div className="mc-grid" aria-label={`Money charts in ${c}`}>
      <section className="mc-card" aria-labelledby={`mc-ie-${c}`}>
        <h3 id={`mc-ie-${c}`}>Income and spending by month ({c})</h3>
        {enough(ie.income) || enough(ie.expense) ? (
          <>
            <div className="mc-stack">
              {([["Income", ie.income], ["Spent", ie.expense]] as const).map(([name, vals], k) => (
                <div key={name} className="mc-row" data-k={k === 0 ? "income" : "spent"}><span className="mc-row__k">{name}</span>
                  <TrendChart kind="bar" values={vals} labels={ie.labels} lastLabel={ie.labels[ie.labels.length - 1]} hideX={k === 0} height={k === 0 ? 89 : 102} format={fmt}
                    label={`${name} by month in ${c}`} hover={hover} onHover={setHover} xTicks={ie.labels.map((_, i) => i)}
                    readout={(i) => <><strong>{ie.months[i]}</strong><span>Income {show(ie.income[i])}</span><span>Spent {show(ie.expense[i])}</span></>} />
                </div>
              ))}
            </div>
            <DataTable caption={`Income and spending by month, ${c}`} head={["Month", "Income", "Spent"]} rows={ie.months.map((m, i) => [m, show(ie.income[i]), show(ie.expense[i])])} />
          </>
        ) : <Empty>Needs two months with entries. Months with nothing recorded are left blank, not shown as zero.</Empty>}
      </section>

      <section className="mc-card" aria-labelledby={`mc-cat-${c}`}>
        <h3 id={`mc-cat-${c}`}>Spending by category{cat.month ? `, ${cat.month}` : ""}</h3>
        {cat.rows.length ? (
          <>
            <p className="mc-legend"><span><i data-k="now" />This month so far</span><span><i data-k="usual" />{cat.priorMonths ? `Median of the earlier ${cat.priorMonths} month${cat.priorMonths === 1 ? "" : "s"}` : "No earlier month to compare"}</span></p>
            <ul className="mc-cats" aria-hidden="true">
              {cat.rows.map((r) => (
                <li key={r.category} className="mc-cat" tabIndex={-1}>
                  <span className="mc-cat__name">{CATEGORY_LABEL[r.category] ?? r.category}</span>
                  <span className="mc-cat__bars"><b style={{ width: `${(r.thisMonth / max) * 100}%` }} />{r.usual != null && <i style={{ width: `${(r.usual / max) * 100}%` }} />}</span>
                  <span className="mc-cat__v">{r.thisShown}<small>usual {r.usualShown}</small></span>
                </li>
              ))}
            </ul>
            <DataTable caption={`Spending by category in ${c}`} head={["Category", "This month so far", "Usual (median)"]} rows={cat.rows.map((r) => [CATEGORY_LABEL[r.category] ?? r.category, r.thisShown, r.usualShown])} />
          </>
        ) : <Empty>No categorised spending this month yet.</Empty>}
      </section>

      <section className="mc-card" aria-labelledby={`mc-sav-${c}`}>
        <h3 id={`mc-sav-${c}`}>Savings over time ({c})</h3>
        {sav.values.length >= 2 && sav.values.some((v) => v !== 0) ? (
          <>
            <TrendChart values={sav.values} labels={sav.labels} lastLabel={sav.labels[sav.labels.length - 1]} height={144} format={fmt} label={`Total saved by month in ${c}${sav.targetShown ? `, with a target of ${sav.targetShown}` : ""}`}
              usual={sav.target != null ? { low: sav.target, high: sav.target, median: sav.target } : null} xTicks={sav.labels.map((_, i) => i)}
              readout={(i) => <><strong>{sav.shown[i]}</strong><span>{sav.labels[i]}</span>{sav.targetShown && <span>Target {sav.targetShown}</span>}</>} />
            <p className="mc-legend"><span><i data-k="now" />Total saved</span>{sav.targetShown ? <span><i data-k="target" />Target {sav.targetShown}</span> : <span>No savings goal set</span>}</p>
            <DataTable caption={`Total saved by month, ${c}`} head={["Month", "Total saved"]} rows={sav.labels.map((l, i) => [l, sav.shown[i]])} />
          </>
        ) : <Empty>Add a savings entry in two different months to see this.</Empty>}
      </section>

      <section className="mc-card" aria-labelledby={`mc-bal-${c}`}>
        <h3 id={`mc-bal-${c}`}>Money I have now, over time ({c})</h3>
        {bal.values.length >= 2 ? (
          <>
            <TrendChart values={bal.values} labels={bal.labels} lastLabel={bal.labels[bal.labels.length - 1]} height={144} format={fmt} label={`Balances you entered in ${c}`}
              readout={(i) => <><strong>{bal.shown[i]}</strong><span>{bal.labels[i]}</span></>} />
            <p className="mc-legend"><span>Each point is a balance you entered.</span></p>
            <DataTable caption={`Balances entered, ${c}`} head={["Date", "Balance"]} rows={bal.labels.map((l, i) => [`${l} (${i + 1})`, bal.shown[i]])} />
          </>
        ) : <Empty>Enter your balance on two different days to see this.</Empty>}
      </section>
      <p className="mc-note">Amounts are in {c} only. Nothing here is converted between currencies.</p>
    </div>
  );
}
