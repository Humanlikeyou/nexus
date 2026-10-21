import { money } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { State } from "./shared";
import { Crosshair } from "lucide-react";

export function RevenueField({ data }: { data: SystemSnapshot }) {
  const deployed = data.risk.global_capital_limit ? data.risk.global_capital_deployed / data.risk.global_capital_limit * 100 : 0;
  const isAwaitingRevenue = [
    data.revenue.total_revenue,
    data.revenue.total_expenses,
    data.revenue.net_income,
    data.revenue.today_revenue,
    data.revenue.week_revenue,
    data.revenue.month_revenue,
  ].every((value) => value === 0) && data.revenue.by_strategy.length === 0;

  if (isAwaitingRevenue) {
    return (
      <div className="system-field revenue-field revenue-field--awaiting">
        <section className="revenue-heading"><span className="system-kicker">Revenue system</span><h1>ARM</h1><State value={data.risk.kill_switch_active ? "fault" : "healthy"} label={data.risk.kill_switch_active ? "Kill switch engaged" : "Risk envelope clear"} /></section>
        <section className="revenue-zero-state">
          <div className="revenue-zero-copy"><span className="system-kicker">REVENUE STATE</span><h2>Awaiting the first revenue event.</h2><p>There is no recorded revenue or expense activity in the system yet. The operating envelope is ready and will begin tracking when a real source reports in.</p></div>
          <div className="revenue-zero-metrics"><div><span>Revenue</span><strong>{money(data.revenue.total_revenue)}</strong></div><div><span>Net income</span><strong>{money(data.revenue.net_income)}</strong></div><div><span>Capital deployed</span><strong>{money(data.risk.global_capital_deployed)}<small> of {money(data.risk.global_capital_limit)}</small></strong></div><div><span>Active strategies</span><strong>{data.risk.active_strategies}</strong></div></div>
        </section>
        <aside className="revenue-zero-strategies"><header><span>Configured strategies</span><strong>{data.strategies.length}</strong></header>{data.strategies.length > 0 ? data.strategies.map((strategy) => <article key={strategy.id}><State value={strategy.status} /><strong>{strategy.name}</strong><span>{strategy.type.replace(/_/g, " ")}</span><small>{money(strategy.capital_limit)} capital limit</small></article>) : <p>No revenue strategy has been configured.</p>}</aside>
      </div>
    );
  }

  return (
    <div className="system-field revenue-field">
      <section className="revenue-heading"><span className="system-kicker">Revenue system</span><h1>ARM</h1><State value={data.risk.kill_switch_active ? "fault" : "healthy"} label={data.risk.kill_switch_active ? "Kill switch engaged" : "Risk envelope clear"} /></section>
      <section className="revenue-hero"><div><span>TOTAL REVENUE</span><strong>{money(data.revenue.total_revenue)}</strong></div><div><span>NET INCOME</span><strong>{money(data.revenue.net_income)}</strong></div></section>
      <section className="capital-vector"><Crosshair size={96} /><div><span>CAPITAL DEPLOYED</span><strong>{deployed.toFixed(0)}<small>%</small></strong><i><b style={{ width: `${Math.min(100, deployed)}%` }} /></i><p>{money(data.risk.global_capital_deployed)} of {money(data.risk.global_capital_limit)}</p></div></section>
      <section className="period-vector"><div><span>Today</span><strong>{money(data.revenue.today_revenue)}</strong></div><div><span>7 days</span><strong>{money(data.revenue.week_revenue)}</strong></div><div><span>30 days</span><strong>{money(data.revenue.month_revenue)}</strong></div><div><span>Expenses</span><strong>{money(data.revenue.total_expenses)}</strong></div></section>
      <section className="strategy-stream"><header><span>ACTIVE STRATEGY MATRIX</span><strong>{data.strategies.length}</strong></header>{data.strategies.map((strategy) => <article key={strategy.id}><State value={strategy.status} /><strong>{strategy.name}</strong><span>{strategy.type.replace(/_/g, " ")}</span><span>{money(strategy.capital_limit)} deployed</span><em>{money(strategy.total_revenue - strategy.total_expenses)}</em></article>)}</section>
      <aside className="revenue-mix"><header>REVENUE MIX</header>{data.revenue.by_strategy.map((item) => <div key={item.strategy_id}><span>{item.strategy_name}</span><i><b style={{ width: `${data.revenue.total_revenue ? item.revenue / data.revenue.total_revenue * 100 : 0}%` }} /></i><strong>{money(item.net)}</strong></div>)}</aside>
    </div>
  );
}
