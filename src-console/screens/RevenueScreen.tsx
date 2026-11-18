import { RefreshCw } from "lucide-react";
import { businessAdapter, type BusinessAdapter } from "../business/adapters";
import { money } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Button, Card, DataTag, PageHero } from "../ui";
import type { Route } from "../app/routes";
import { LinkCard, PERSONAL_ITEMS, waitingDecisions } from "./BusinessPanels";
import { valueOf } from "../lib/sampleAdapters";
import { PageTake } from "../design1118/PageTake";
import "../design1118/design1118.css";
import "./BusinessScreen.css";
import "./CommandCenter.css";

/** Enterprise › Finances (was Business › Money; design-1110d D-B9–B11, design-1111v). Read-only from the business ledger
 *  (read_business_ledger). USD only; trading risk lives in Ventures. Votes and flags are handled in Decisions. */
function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="biz__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

const acc = (node: React.ReactNode) => <span className="cc-card__acc">{node}</span>;

export function RevenueField({ data, onRetry, onOpen, adapter = businessAdapter(data) }: { data: SystemSnapshot; onRetry?: () => void; onOpen?: (route: Route) => void; adapter?: BusinessAdapter }) {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const { revenue } = data;
  const isAwaitingRevenue = [revenue.total_revenue, revenue.total_expenses, revenue.net_income, revenue.today_revenue, revenue.week_revenue, revenue.month_revenue]
    .every((value) => value === 0) && revenue.by_strategy.length === 0;
  const summaryDown = down("arm_get_revenue_summary") && isAwaitingRevenue;
  const funds = adapter.funds();
  const waiting = waitingDecisions(adapter);
  const top = Math.max(1, ...revenue.by_strategy.map((b) => Math.max(b.revenue, b.expenses)));
  // design-1118a: fund rows stay hidden until a fund has a balance; Akiya is personal and never in this ledger.
  const fundList = (valueOf(funds) ?? []).filter((f) => !PERSONAL_ITEMS.has(f.name.toLowerCase()));
  const withBalance = fundList.filter((f) => f.balanceUsd.state !== "unwired" && f.balanceUsd.value > 0);
  const total = withBalance.reduce((n, f) => n + (f.balanceUsd.state === "unwired" ? 0 : f.balanceUsd.value), 0);
  const needsConversion = fundList.some((f) => f.balanceUsd.state === "unwired" && f.balanceUsd.reason === "currency");
  const empty = isAwaitingRevenue && withBalance.length === 0 && !summaryDown;

  return (
    <div className="biz page-inset cc-page d18-page">
      <PageHero title="Finances" statement={summaryDown ? "Revenue is out of reach" : isAwaitingRevenue ? "No revenue yet" : `${money(revenue.net_income)} net in 30 days`}
        detail={<>{isAwaitingRevenue ? "Tracking starts when a real source reports in." : `On ${money(revenue.total_revenue)} revenue and ${money(revenue.total_expenses)} costs. Amounts in USD; ledger values in yen wait for currency conversion.`} Read-only, from the business ledger. Personal money is in {onOpen ? <button type="button" className="v-link" onClick={() => onOpen({ id: "life", view: "money" })}>Life › Personal money ›</button> : "Life › Personal money"}.</>} />
      <PageTake surface="finances" className="biz__take" />
      {summaryDown && <Unavailable label="Revenue summary" onRetry={onRetry} />}
      {empty ? (
        /* design-1118a: an honest empty state. No rows of dashes; funds appear once one has a balance. */
        <section className="d18-card d18-empty" aria-labelledby="fin-empty">
          <div className="d18-mring" data-tone="mint" style={{ width: 96, height: 96 }} role="img" aria-label="0% revenue">
            <svg width="96" height="96" viewBox="0 0 96 96" aria-hidden="true"><circle className="d18-mring__track" cx="48" cy="48" r="41" /></svg>
            <div className="d18-mring__v" aria-hidden="true"><b className="d18-mring__mid">0%</b><small>revenue</small></div>
          </div>
          <h2 id="fin-empty" className="d18-route__t">No revenue yet</h2>
          <p className="d18-note">When a source reports in, a 30-day path and fund vessels will fill here.{needsConversion ? <> Ledger balances are in yen <DataTag kind="currency" /></> : null}</p>
          <p className="d18-note">The ledger is read-only. Funding votes and tax flags stay in Decisions. Akiya and other personal money stay in Life › Personal money.</p>
        </section>
      ) : (
        <div className="cc-grid2">
          <Card title="Venture funds" accessory={acc(funds.state === "sample" ? <DataTag kind="sample" /> : withBalance.length ? null : <DataTag kind="wire" />)}>
            {withBalance.length === 0 ? <p className="cc-note">Fund balances appear here once the ledger reports one.{needsConversion ? <> Ledger balances are in yen <DataTag kind="currency" /></> : null}</p> : (
              <ul className="d18-vessels" aria-label="Fund balances">
                {withBalance.map((f) => { const v = (f.balanceUsd as { value: number }).value; const frac = total ? v / total : 0; return (
                  <li key={f.name}>
                    <div className="d18-mring" data-tone="mint" style={{ width: 72, height: 72 }} role="img" aria-label={`${f.name} ${money(v)}, ${Math.round(frac * 100)}% of funds`}>
                      <svg width="72" height="72" viewBox="0 0 72 72" aria-hidden="true"><circle className="d18-mring__track" cx="36" cy="36" r="29" /><circle className="d18-mring__fill" cx="36" cy="36" r="29" strokeDasharray={`${(frac * 182.2).toFixed(1)} 182.2`} transform="rotate(-90 36 36)" /></svg>
                      <div className="d18-mring__v" aria-hidden="true"><b>{money(v)}</b></div>
                    </div>
                    <b>{f.name}</b>
                  </li>
                ); })}
              </ul>
            )}
          </Card>
          <Card title="Income & costs · 30 days">
            {revenue.by_strategy.length === 0 ? <p className="cc-note">{isAwaitingRevenue ? "No strategy has reported revenue yet." : "No per-strategy breakdown."}</p> : (
              <div className="cc-bars" role="table" aria-label="Revenue and costs by strategy">
                {revenue.by_strategy.map((b) => (
                  <div className="cc-bars__row" role="row" key={b.strategy_id}>
                    <span role="rowheader">{b.strategy_name}</span>
                    <span className="cc-bars__track" role="cell" aria-label={`${money(b.revenue)} income, ${money(b.expenses)} costs`}><i className="cc-bars__in" style={{ width: `${(b.revenue / top) * 100}%` }} /><i className="cc-bars__out" style={{ width: `${(b.expenses / top) * 100}%` }} /></span>
                    <span className="cc-num" role="cell">{money(b.revenue)} / {money(b.expenses)}</span>
                  </div>
                ))}
              </div>
            )}
            <p className="cc-note">Solid = income · striped = costs · today {money(revenue.today_revenue)} in, {money(revenue.today_expenses)} out</p>
          </Card>
        </div>
      )}
      <LinkCard title="In Decisions" line={waiting.unwired ? "Funding votes and tax flags from the ledger are handled in Decisions." : `${waiting.open.length} funding vote${waiting.open.length === 1 ? "" : "s"} and ${waiting.tax.length} tax flag${waiting.tax.length === 1 ? "" : "s"} are waiting. Votes and flags are handled in one place.`}
        open="Open Decisions ›" onOpen={onOpen ? () => onOpen({ id: "enterprise", view: "decisions" }) : undefined} tag={waiting.unwired ? <DataTag kind="wire" /> : waiting.sample ? <DataTag kind="sample" /> : null} />
    </div>
  );
}
