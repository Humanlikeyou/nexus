import { RefreshCw } from "lucide-react";
import { humanize, money } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Badge, Button, Card, Figure, Figures, InsetGroup, ListRow, Meter, More, PageHero } from "../ui";
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

  const deployedPct = risk.global_capital_limit ? (risk.global_capital_deployed / risk.global_capital_limit) * 100 : null;

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

  const details = (title: string) => (
    <More label="Strategies, capital and risk">
      {!riskDown && deployedPct != null && <Figures><Figure label="Capital deployed" value={`${Math.round(deployedPct)}%`} /><Figure label="Of limit" value={`${money(risk.global_capital_deployed)} of ${money(risk.global_capital_limit)}`} /></Figures>}
      {strategyList(title)}
      {revenue.by_strategy.length > 0 && (
        <InsetGroup title="Revenue by strategy">
          {revenue.by_strategy.map((item) => (
            <ListRow key={item.strategy_id} title={item.strategy_name} value={money(item.net)}
              accessory={<Meter className="biz__mix-meter" size="sm" label={`${item.strategy_name} share of revenue`} value={item.revenue} max={revenue.total_revenue || 1} />} />
          ))}
        </InsetGroup>
      )}
      {riskFooter}
    </More>
  );

  if (down("arm_get_revenue_summary") && isAwaitingRevenue) {
    return (
      <div className="biz page-inset">
        <PageHero title="Business" statement="Revenue is out of reach" detail={status} />
        <Unavailable label="Revenue summary" onRetry={onRetry} />
        {details("Configured strategies")}
      </div>
    );
  }

  if (isAwaitingRevenue) {
    return (
      <div className="biz page-inset">
        <PageHero title="Business" statement="No revenue yet" detail="Tracking starts when a real source reports in." />
        {status}
        {details("Configured strategies")}
      </div>
    );
  }

  const net = revenue.net_income;
  return (
    <div className="biz page-inset">
      <PageHero title="Business" statement={money(net)} detail={<>net income, on {money(revenue.total_revenue)} revenue</>} />
      <Figures className="biz__figures">
        <Figure label="Today" value={money(revenue.today_revenue)} />
        <Figure label="7 days" value={money(revenue.week_revenue)} />
        <Figure label="30 days" value={money(revenue.month_revenue)} />
        <Figure label="Expenses" value={money(revenue.total_expenses)} />
      </Figures>
      {status}
      {details("Strategies")}
    </div>
  );
}
