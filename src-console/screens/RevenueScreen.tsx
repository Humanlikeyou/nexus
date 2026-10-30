import { RefreshCw } from "lucide-react";
import { humanize, money } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, Meter, Ring, StatTile } from "../ui";
import { badgeTone } from "./businessTone";
import "./BusinessScreen.css";

/** Business › Revenue (screen-specs §8). The kill switch is display-only (OQ-E). */
function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="biz__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

export function RevenueField({ data, onRetry }: { data: SystemSnapshot; onRetry?: () => void }) {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const { revenue, risk, strategies } = data;
  const riskDown = down("arm_get_risk_status");
  const isAwaitingRevenue = [revenue.total_revenue, revenue.total_expenses, revenue.net_income, revenue.today_revenue, revenue.week_revenue, revenue.month_revenue]
    .every((value) => value === 0) && revenue.by_strategy.length === 0;

  const status = riskDown ? null : risk.kill_switch_active
    ? <Badge tone="danger">Kill switch engaged</Badge>
    : <Badge tone="success">Risk within limits</Badge>;

  const strategyList = (title: string) => down("arm_list_strategies") && strategies.length === 0
    ? <Card title={title}><Unavailable label="Revenue strategies" onRetry={onRetry} /></Card>
    : strategies.length === 0
      ? <Card title={title}><p className="biz__note"><strong>No strategies yet</strong><br />Strategies appear here once you configure one.</p></Card>
      : (
        <InsetGroup title={`${title} (${strategies.length})`}>
          {strategies.map((s) => (
            <ListRow key={s.id} title={s.name} subtitle={`${humanize(s.type)} · ${money(s.capital_limit)} limit`} value={money(s.total_revenue - s.total_expenses)}
              accessory={<Badge tone={badgeTone(s.status)} title={s.health_status ? `Health: ${humanize(s.health_status)}` : undefined}>{humanize(s.status)}</Badge>} />
          ))}
        </InsetGroup>
      );

  const riskFooter = riskDown ? <Unavailable label="Revenue risk" onRetry={onRetry} /> : (
    <div className="biz__risk">
      <p>Daily P&amp;L {money(risk.global_daily_pnl)} of {money(risk.global_daily_loss_cap)} loss cap · Risk index {risk.risk_index}</p>
      {risk.strategies_at_risk.length > 0 && <div className="biz__at-risk">{risk.strategies_at_risk.map((name) => <Badge key={name} tone="warning">{name}</Badge>)}</div>}
    </div>
  );

  if (down("arm_get_revenue_summary") && isAwaitingRevenue) {
    return (
      <div className="biz">
        <div className="biz__head"><h1 className="biz__title">Business</h1>{status}</div>
        <Card title="Revenue"><Unavailable label="Revenue summary" onRetry={onRetry} /></Card>
        {strategyList("Configured strategies")}
        {riskFooter}
      </div>
    );
  }

  if (isAwaitingRevenue) {
    return (
      <div className="biz">
        <div className="biz__head"><h1 className="biz__title">Business</h1>{status}</div>
        <Card title="No revenue yet"><p className="biz__note">Tracking starts when a real source reports in.</p></Card>
        {strategyList("Configured strategies")}
        {riskFooter}
      </div>
    );
  }

  const net = revenue.net_income;
  const deployedPct = risk.global_capital_limit ? (risk.global_capital_deployed / risk.global_capital_limit) * 100 : null;
  return (
    <div className="biz">
      <div className="biz__head"><h1 className="biz__title">Business</h1>{status}</div>
      <div className="biz__hero">
        <StatTile label="Total revenue" value={money(revenue.total_revenue)} />
        <StatTile label="Net income" value={money(net)}
          trend={{ direction: net > 0 ? "up" : net < 0 ? "down" : "flat", label: net > 0 ? "Positive net income" : net < 0 ? "Negative net income" : "Break-even", sentiment: net > 0 ? "positive" : net < 0 ? "negative" : "neutral" }} />
        <Card title="Capital deployed">
          {riskDown ? <Unavailable label="Revenue risk" onRetry={onRetry} /> : (
            <div className="biz__ring">
              <Ring value={deployedPct} label="Capital deployed" valueText={deployedPct == null ? "No limit set" : `${Math.round(deployedPct)}%`}>
                <span className="biz__ring-value">{deployedPct == null ? "—" : `${Math.round(deployedPct)}%`}</span>
              </Ring>
              <p className="biz__note">{money(risk.global_capital_deployed)} of {money(risk.global_capital_limit)}</p>
            </div>
          )}
        </Card>
      </div>
      <div className="biz__tiles">
        <StatTile label="Today" value={money(revenue.today_revenue)} />
        <StatTile label="7 days" value={money(revenue.week_revenue)} />
        <StatTile label="30 days" value={money(revenue.month_revenue)} />
        <StatTile label="Expenses" value={money(revenue.total_expenses)} />
      </div>
      {strategyList("Strategies")}
      {revenue.by_strategy.length > 0 && (
        <InsetGroup title="Revenue by strategy">
          {revenue.by_strategy.map((item) => (
            <ListRow key={item.strategy_id} title={item.strategy_name} value={money(item.net)}
              accessory={<Meter className="biz__mix-meter" size="sm" label={`${item.strategy_name} share of revenue`} value={item.revenue} max={revenue.total_revenue || 1} />} />
          ))}
        </InsetGroup>
      )}
      {riskFooter}
    </div>
  );
}
