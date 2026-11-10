import { RefreshCw } from "lucide-react";
import { businessAdapter, type BusinessAdapter } from "../business/adapters";
import { money } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Button, Card, DataTag, PageHero, StatusText, wiredTag } from "../ui";
import "./BusinessScreen.css";
import "./CommandCenter.css";

/** Business › Money (design-1110d D-B9–B11). USD only; trading risk lives in Ventures (approval answer 2). */
function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="biz__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

const acc = (node: React.ReactNode) => <span className="cc-card__acc">{node}</span>;

export function RevenueField({ data, onRetry, adapter = businessAdapter() }: { data: SystemSnapshot; onRetry?: () => void; adapter?: BusinessAdapter }) {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const { revenue } = data;
  const isAwaitingRevenue = [revenue.total_revenue, revenue.total_expenses, revenue.net_income, revenue.today_revenue, revenue.week_revenue, revenue.month_revenue]
    .every((value) => value === 0) && revenue.by_strategy.length === 0;
  const summaryDown = down("arm_get_revenue_summary") && isAwaitingRevenue;
  const funds = adapter.funds();
  const votes = adapter.fundingRequests();
  const rules = adapter.voteRules();
  const flags = adapter.taxFlags();
  const top = Math.max(1, ...revenue.by_strategy.map((b) => Math.max(b.revenue, b.expenses)));

  return (
    <div className="biz page-inset cc-page">
      <PageHero title="Business" statement={summaryDown ? "Revenue is out of reach" : isAwaitingRevenue ? "No revenue yet" : `${money(revenue.net_income)} net in 30 days`}
        detail={isAwaitingRevenue ? "Tracking starts when a real source reports in." : `On ${money(revenue.total_revenue)} revenue and ${money(revenue.total_expenses)} costs. Amounts in USD; ledger values in yen wait for currency conversion.`} />
      {summaryDown && <Unavailable label="Revenue summary" onRetry={onRetry} />}
      <div className="cc-grid2">
        <Card title="Fund balances" accessory={acc(funds.state === "sample" ? <DataTag kind="sample" /> : <DataTag kind="wire" />)}>
          {funds.state === "unwired" ? <p className="cc-note">Ledger funds appear here.</p> : (
            <table className="cc-table" aria-label="Fund balances">
              <thead><tr><th scope="col">Fund</th><th scope="col" className="cc-num">Balance</th></tr></thead>
              <tbody>
                {funds.value.map((f) => <tr key={f.name}><td>{f.name}</td><td className="cc-num">{f.balanceUsd.state === "unwired" ? "\u2014" : money(f.balanceUsd.value)}</td></tr>)}
                <tr><td><b>Total</b></td><td className="cc-num"><b>{funds.value.every((f) => f.balanceUsd.state !== "unwired") ? money(funds.value.reduce((n, f) => n + (f.balanceUsd.state === "unwired" ? 0 : f.balanceUsd.value), 0)) : "\u2014"}</b></td></tr>
              </tbody>
            </table>
          )}
          {funds.state !== "unwired" && funds.value.some((f) => f.balanceUsd.state === "unwired") && <p className="cc-note">Ledger balances are in yen <DataTag kind="currency" /></p>}
        </Card>
        <Card title="Income and costs · 30 days">
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
      <Card title="Funding requests and votes" accessory={acc(wiredTag(votes))}>
        {votes.state === "unwired" ? <p className="cc-note">Requests, purposes and vote results from the ledger appear here.</p> : (
          <table className="cc-table" aria-label="Funding requests">
            <thead><tr><th scope="col">Requester</th><th scope="col">Purpose</th><th scope="col" className="cc-num">Amount</th><th scope="col">Vote</th></tr></thead>
            <tbody>{votes.value.map((v) => <tr key={v.id}><td>{v.requester}</td><td>{v.purpose}</td><td className="cc-num">{v.amountUsd.state === "unwired" ? "\u2014" : money(v.amountUsd.value)}</td><td><StatusText kind={v.status === "open" ? "wait" : v.status === "approved" ? "ok" : "fail"}>{v.status === "open" ? "Open" : v.status === "approved" ? "Approved" : "Denied"}</StatusText></td></tr>)}</tbody>
          </table>
        )}
        {votes.state !== "unwired" && <p className="cc-note">Amounts <DataTag kind="currency" /> · vote rules {rules.state === "unwired" ? <DataTag kind="wire" /> : `${rules.value.needed} yes votes`}</p>}
      </Card>
      <Card title={flags.state === "unwired" ? "Tax flags" : `Tax flags \u00b7 ${flags.value.length}`} accessory={acc(wiredTag(flags))}>
        {flags.state === "unwired" ? <p className="cc-note">Ledger tax flags appear here.</p> : flags.value.map((t) => (
          <div className="cc-row" key={t.id}><StatusText kind="risk" /><div className="cc-row__main"><span>{t.label}</span><small>{t.area} · {t.date}</small></div><Button size="small" variant="secondary">Open</Button></div>
        ))}
      </Card>
    </div>
  );
}
