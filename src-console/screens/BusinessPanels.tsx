import { ChevronLeft } from "lucide-react";
import { businessAdapter, type BusinessAdapter, type Health, type Venture } from "../business/adapters";
import { humanize, money } from "../lib/format";
import { valueOf, type Wired } from "../lib/sampleAdapters";
import type { SystemSnapshot } from "../types";
import { Button, Card, DataTag, Meter, PageHero, StatusText, wiredTag, type StatusKind } from "../ui";
import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import { LinkButton, OffButton } from "../adds/Composer";
import { decisionsLog, jst } from "../decisions/log";
import { PageTake } from "../design1118/PageTake";
import { GoldenUnderlay } from "../design1118/GoldenUnderlay";
import { MarketNewsCards, ProposalsCard, TradingCard } from "../design1118/Trading";
import "./CommandCenter.css";
import "../adds/v1111.css";

const HEALTH: Record<Health, { kind: StatusKind; text: string }> = {
  on_track: { kind: "off", text: "On track" }, at_risk: { kind: "risk", text: "At risk" }, off_track: { kind: "fail", text: "Off track" },
  done: { kind: "ok", text: "Done" }, waiting: { kind: "wait", text: "Waiting on vote" }, searching: { kind: "off", text: "Searching" },
};
const acc = (node: React.ReactNode) => <span className="cc-card__acc">{node}</span>;
const usd = (w: Wired<number>) => (w.state === "unwired" ? "\u2014" : money(w.value));

/** True when the revenue engine has reported anything (all-zero = not set up yet). */
const hasRevenue = (r: SystemSnapshot["revenue"]) => !([r.total_revenue, r.total_expenses, r.net_income, r.today_revenue, r.week_revenue, r.month_revenue].every((v) => v === 0) && r.by_strategy.length === 0);

/** D-B5 + approval answer 2: Trading is a venture, from the revenue engine's live risk and strategies. */
export function TradingVenture({ data }: { data: SystemSnapshot }) {
  const { risk, strategies, revenue } = data;
  const riskDown = data.unavailableSources.some((s) => s.command === "arm_get_risk_status");
  const net = (id: string) => revenue.by_strategy.find((b) => b.strategy_id === id)?.net;
  return (
    <div className="cc-venture">
      <h3>Trading {riskDown ? <StatusText kind="off">Risk unavailable</StatusText> : risk.kill_switch_active ? <StatusText kind="fail">Kill switch engaged</StatusText> : <StatusText kind="ok">Within limits</StatusText>}</h3>
      {!riskDown && <>
        <p>{risk.active_strategies} active strateg{risk.active_strategies === 1 ? "y" : "ies"} · daily P&amp;L {money(risk.global_daily_pnl)} of {money(risk.global_daily_loss_cap)} loss cap · kill switch {risk.kill_switch_active ? "on" : "off"}</p>
        <Meter size="sm" label={`Capital deployed ${money(risk.global_capital_deployed)} of ${money(risk.global_capital_limit)}`} value={risk.global_capital_deployed} max={risk.global_capital_limit || 1} />
        <p>Capital {money(risk.global_capital_deployed)} of {money(risk.global_capital_limit)} deployed · risk index {risk.risk_index}</p>
      </>}
      {revenue.by_strategy.length > 0 && <p>{revenue.by_strategy.map((b) => { const s = strategies.find((x) => x.id === b.strategy_id); return <span key={b.strategy_id} className="cc-strategy">{b.strategy_name}{s?.health_status ? <> <StatusText kind={s.health_status === "healthy" ? "ok" : "risk"}>{humanize(s.health_status)}</StatusText></> : null} {money(net(b.strategy_id) ?? b.net)} net</span>; })}</p>}
    </div>
  );
}

function VentureCell({ v, w, onOpen }: { v: Venture; w: Wired<Venture[]>; onOpen?: (id: string) => void }) {
  const h = HEALTH[v.health];
  return (
    <div className="cc-venture">
      <h3>{onOpen ? <button type="button" className="cc-venture__open" onClick={() => onOpen(v.id)}>{v.name}</button> : v.name} <StatusText kind={h.kind}>{h.text}</StatusText> {wiredTag(w)}</h3>
      <p>{[v.stage, v.progress ? `${v.progress.done} of ${v.progress.total} ${v.progress.label}` : null, v.note || null].filter(Boolean).join(" · ")}{v.target.state === "unwired" && v.target.reason === "currency" ? <> · target price <DataTag kind="currency" /></> : null}</p>
      {v.progress && <Meter size="sm" className="cc-meter--warn" label={`${v.name}: ${v.progress.done} of ${v.progress.total} ${v.progress.label}`} value={v.progress.done} max={v.progress.total} color={v.health === "at_risk" ? "var(--copper)" : undefined} />}
      {v.nextStep && <p>Next: {v.nextStep}</p>}
    </div>
  );
}

/** design-1111v (Taisun, 19:23 JST): Akiya is a personal item now and lives in Life, not in Ventures. */
export const PERSONAL_ITEMS = new Set(["akiya"]);
const enterpriseVentures = (w: Wired<Venture[]>): Wired<Venture[]> => (w.state === "unwired" ? w : { ...w, value: w.value.filter((v) => !PERSONAL_ITEMS.has(v.id)) });

export function VenturesCard({ data, adapter, onOpen }: { data: SystemSnapshot; adapter: BusinessAdapter; onOpen?: (id: string) => void }) {
  const ventures = enterpriseVentures(adapter.ventures());
  return (
    <Card title="Ventures" accessory={acc(<span>Health · next step</span>)}>
      <div className="cc-grid2">
        {ventures.state !== "unwired" && ventures.value.slice(0, 1).map((v) => <VentureCell key={v.id} v={v} w={ventures} onOpen={onOpen} />)}
        <TradingVenture data={data} />
        {ventures.state !== "unwired" && ventures.value.slice(1).map((v) => <VentureCell key={v.id} v={v} w={ventures} onOpen={onOpen} />)}
      </div>
      {ventures.state === "unwired" && <p className="cc-note">Taiyo Skin and other ventures appear here once venture tracking is connected <DataTag kind="wire" /></p>}
    </Card>
  );
}

/** Overview's one-line link card (design-1111v): what is waiting, and a link to the page that handles it. */
export function LinkCard({ title, line, open, onOpen, tag }: { title: string; line: React.ReactNode; open: string; onOpen?: () => void; tag?: React.ReactNode }) {
  return (
    <Card title={title} accessory={acc(tag)}>
      <div className="v-linkrow"><p>{line}</p><span className="v-grow" /><LinkButton onClick={onOpen}>{open}</LinkButton></div>
    </Card>
  );
}

/** Open votes and tax flags with no record in the decisions log. */
export function waitingDecisions(adapter: BusinessAdapter) {
  const votes = adapter.fundingRequests(), flags = adapter.taxFlags();
  const decided = new Set((valueOf(decisionsLog().list()) ?? []).map((r) => r.itemId));
  const open = (valueOf(votes) ?? []).filter((v) => v.status === "open" && !decided.has(v.id));
  const tax = (valueOf(flags) ?? []).filter((t) => !decided.has(t.id));
  const unwired = votes.state === "unwired" && flags.state === "unwired";
  const sample = votes.state === "sample" || flags.state === "sample";
  return { open, tax, n: open.length + tax.length, unwired, sample };
}

export function DecisionsCard({ adapter, onOpen }: { adapter: BusinessAdapter; onOpen?: () => void }) {
  const w = waitingDecisions(adapter);
  const line = w.unwired ? "Funding votes and tax flags from the ledger appear here." : w.n === 0 ? "Nothing is waiting for a decision." : [...w.open.map((v) => `${v.requester} ${v.purpose.replace(/ \(.*\)$/, "").toLowerCase()} vote`), ...w.tax.map((t) => `tax flag: ${t.label.charAt(0).toLowerCase()}${t.label.slice(1)}`)].join(" · ");
  return <LinkCard title={w.unwired ? "Decisions" : `Decisions · ${w.n} waiting`} line={line} open="Open Decisions ›" onOpen={onOpen} tag={w.unwired ? <DataTag kind="wire" /> : w.sample ? <DataTag kind="sample" /> : null} />;
}

/** Enterprise › Overview (D-B2–B6, design-1111v): the take and recommendation stay here; Decisions, Tasks and Content are link cards. */
export function BusinessOverview({ data, adapter = businessAdapter(data), onOpenVenture, onView, work }: { data: SystemSnapshot; adapter?: BusinessAdapter; onOpenVenture: (id: string) => void; onView?: (view: string) => void; work: React.ReactNode }) {
  const { revenue, risk } = data;
  const riskDown = data.unavailableSources.some((s) => s.command === "arm_get_risk_status");
  const target = adapter.revenueTargetUsd();
  const ventures = enterpriseVentures(adapter.ventures());
  const atRisk = ventures.state === "unwired" ? [] : ventures.value.filter((v) => v.health === "at_risk");
  const decisions = waitingDecisions(adapter).n;
  const sampleLine = [decisions ? `${decisions} need a decision` : null, atRisk.length ? `${atRisk.map((v) => v.name).join(", ")} at risk` : null].filter(Boolean).join(", ");
  const live = hasRevenue(revenue);
  return (
    <div className="biz page-inset cc-page">
      <PageHero title="Enterprise" statement={live ? `${money(revenue.net_income)} net in 30 days` : "No revenue yet"}
        detail={<>{live ? `On ${money(revenue.total_revenue)} revenue` : "Tracking starts when a real source reports in"}{riskDown ? "" : ` · risk ${risk.kill_switch_active ? "kill switch engaged" : "within limits"}`}{sampleLine ? <> · {sampleLine} <span className="cc-hero-tag"><DataTag kind="sample" /></span></> : null}</>} />
      <ul className="cc-strip" aria-label="Revenue, last 30 days">
        <li className="cc-tile"><span className="cc-tile__k">Revenue · 30 days</span><span className="cc-tile__v">{money(revenue.month_revenue)}</span><span className="cc-tile__s">Target {target.state === "unwired" ? wiredTag(target) : money(target.value)}</span></li>
        <li className="cc-tile"><span className="cc-tile__k">Net income</span><span className="cc-tile__v">{money(revenue.net_income)}</span><span className="cc-tile__s">after {money(revenue.total_expenses)} costs</span></li>
        <li className="cc-tile"><span className="cc-tile__k">This week</span><span className="cc-tile__v">{money(revenue.week_revenue)}</span><span className="cc-tile__s">Today {money(revenue.today_revenue)}</span></li>
        <li className="cc-tile"><span className="cc-tile__k">Trading risk</span><span className="cc-tile__v">{riskDown ? <StatusText kind="off">Unavailable</StatusText> : risk.kill_switch_active ? <StatusText kind="fail">Kill switch</StatusText> : <StatusText kind="ok">Within limits</StatusText>}</span><span className="cc-tile__s">{riskDown ? "" : `Risk index ${risk.risk_index} · kill switch ${risk.kill_switch_active ? "on" : "off"}`}</span></li>
      </ul>
      <AmaraTake page="business" className="biz__take" />
      <DecisionsCard adapter={adapter} onOpen={onView ? () => onView("decisions") : undefined} />
      <VenturesCard data={data} adapter={adapter} onOpen={onOpenVenture} />
      {work}
    </div>
  );
}

/** Enterprise › Ventures (D-B5, D-B7, D-B8): every venture, or one on its own page (design-1111v: route ventures/<id>). */
export function VenturesView({ data, adapter = businessAdapter(data), ventureId, onOpen, onView }: { data: SystemSnapshot; adapter?: BusinessAdapter; ventureId: string | null; onOpen: (id: string | null) => void; onView?: (view: string) => void }) {
  const ventures = enterpriseVentures(adapter.ventures());
  const list = ventures.state === "unwired" ? [] : ventures.value;
  const v = ventureId ? list.find((x) => x.id === ventureId) : undefined;
  const detail = v ? adapter.venture(v.id) : null;
  if (v && detail) {
    const d = detail.state === "unwired" ? null : detail.value;
    const h = HEALTH[v.health];
    const tag = wiredTag(detail);
    // design-1111v §4: the venture page reads the decisions log; nothing copies it.
    const logged = (valueOf(decisionsLog().list()) ?? []).filter((r) => r.sourceLink.view === `ventures/${v.id}`);
    const waiting = waitingDecisions(adapter).open.filter((r) => r.requester === v.name).length;
    return (
      <div className="biz page-inset cc-page">
        <nav className="cc-crumb" aria-label="Breadcrumb"><Button className="cc-back" variant="plain" size="small" icon={ChevronLeft} onClick={() => onOpen(null)}>Ventures</Button> › {v.name}</nav>
        <PageHero title={`Venture · ${v.stage.toLowerCase()}`} statement={`${v.name} is ${h.text.toLowerCase()}`} detail={<>{d?.headline ?? "Venture details need wiring."} <span className="cc-hero-tag">{tag}</span></>} />
        <ul className="cc-strip" aria-label="Venture summary">
          <li className="cc-tile"><span className="cc-tile__k">Tasks done</span><span className="cc-tile__v">{d ? `${d.checklist.filter((c) => c.done).length} of ${d.checklist.length}` : "\u2014"}</span><span className="cc-tile__s">{tag}</span></li>
          <li className="cc-tile"><span className="cc-tile__k">Budget used</span><span className="cc-tile__v">{d ? usd(d.budgetUsedUsd) : "\u2014"}</span><span className="cc-tile__s">{d ? wiredTag(d.budgetUsedUsd) : tag}</span></li>
          <li className="cc-tile"><span className="cc-tile__k">Fund balance</span><span className="cc-tile__v">{d ? usd(d.fundUsd) : "\u2014"}</span><span className="cc-tile__s">{d ? wiredTag(d.fundUsd) : tag}</span></li>
          <li className="cc-tile"><span className="cc-tile__k">Waiting decisions</span><span className="cc-tile__v">{d ? waiting : "\u2014"}</span><span className="cc-tile__s">{tag}</span></li>
        </ul>
        {d && <>
          <div className="cc-grid2">
            <Card title="Launch tasks" accessory={acc(tag)}><ul className="cc-check">{d.checklist.map((c) => <li key={c.label} data-done={c.done || undefined}><span className="cc-sr">{c.done ? "Done: " : "To do: "}</span>{c.label}</li>)}</ul></Card>
            <Card title="Milestones" accessory={acc(tag)}>
              {d.milestones.map((m) => <div className="cc-row" key={m.label}><div className="cc-row__main"><span>{m.label}</span><small>{m.due}</small></div><StatusText kind={HEALTH[m.health].kind}>{HEALTH[m.health].text}</StatusText></div>)}
              {logged.map((r) => <div className="cc-row" key={r.id}><div className="cc-row__main"><span>Decided: {r.title.replace(/^[^·]+· /, "")}</span><small>{jst(r.decidedAt)} JST · from the decisions log</small></div><StatusText kind="ok">{r.choice === "handled" ? "Handled" : r.choice === "yes" ? "Yes" : "No"}</StatusText></div>)}
            </Card>
          </div>
          <Card title="Next steps" accessory={acc(tag)}>{d.nextActions.map((a, i) => (
            <div className="cc-row" key={a.label}><div className="cc-row__main"><span>{a.label}</span><small>{a.detail}</small></div>
              {a.action === "Draft with crew" ? <OffButton variant={i === 0 ? "primary" : "secondary"} reason="Needs wiring · crew drafts from a venture">{a.action}</OffButton>
                : a.action === "Open vote" ? <LinkButton small onClick={onView ? () => onView("decisions") : undefined}>Open in Decisions ›</LinkButton>
                  : <OffButton reason="Needs wiring">{a.action}</OffButton>}
            </div>
          ))}</Card>
        </>}
      </div>
    );
  }
  const atRisk = list.filter((x) => x.health === "at_risk").length;
  const riskDown = data.unavailableSources.some((s) => s.command === "arm_get_risk_status");
  return (
    <div className="biz page-inset cc-page d18-page d18-golden">
      <GoldenUnderlay />
      <PageHero title="Ventures" statement={ventures.state === "unwired" ? "Trading" : `${list.length + 1} ventures${atRisk ? `, ${atRisk} at risk` : ""}`}
        detail={<>Trading from the revenue engine&apos;s live data. Proposals are watch-only until you approve a record of your choice — never an order.{list.length ? <> {list.map((x) => x.name).join(" and ")} <span className="cc-hero-tag">{wiredTag(ventures)}</span></> : null}</>} />
      <PageTake surface="ventures" className="biz__take" />
      <TradingCard data={data} />
      <MarketNewsCards />
      <ProposalsCard capUsd={riskDown ? undefined : data.risk.global_daily_loss_cap} />
      <div className="d18-grid2 d18-grid2--end">
        {ventures.state === "unwired" ? (
          <section className="d18-card" aria-labelledby="d18-taiyo">
            <h2 id="d18-taiyo" className="d18-h">Taiyo Skin</h2>
            <div className="d18-route"><svg width="56" height="56" viewBox="0 0 56 56" aria-hidden="true"><g className="d18-seed"><circle className="d18-seed__halo" cx="28" cy="28" r="24" /><circle cx="28" cy="28" r="6" /></g></svg><div><p className="d18-eyebrow d18-eyebrow--quiet">Route</p><p className="d18-route__t">Not connected</p><p className="d18-note">The path appears once venture tracking is connected. <DataTag kind="wire" /></p></div></div>
          </section>
        ) : (
          <Card title="Ventures" accessory={acc(wiredTag(ventures))}>{list.map((x) => <VentureCell key={x.id} v={x} w={ventures} onOpen={(id) => onOpen(id)} />)}</Card>
        )}
        <p className="cc-note">Akiya is a personal item now. Its dates are in {onView ? <LinkButton small onClick={() => onView("life")}>Life › Dates &amp; deadlines</LinkButton> : "Life › Dates & deadlines"}.</p>
      </div>
    </div>
  );
}
