import { useState } from "react";
import { ChevronLeft } from "lucide-react";
import type { Route } from "../app/routes";
import { askMore } from "../amaraTake/takeView";
import { businessAdapter, type BusinessAdapter, type Health, type Venture } from "../business/adapters";
import { money } from "../lib/format";
import { valueOf, type Wired } from "../lib/sampleAdapters";
import type { SystemSnapshot } from "../types";
import { Button, Card, DataTag, InsetGroup, ListRow, Meter, PageHero, StatusText, wiredTag, type StatusKind } from "../ui";
import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import { LinkButton } from "../adds/Composer";
import { decisionsLog, jst } from "../decisions/log";
import { PageTake } from "../design1118/PageTake";
import { VentureGoalsCard } from "../design1118/EnterpriseWiring";
import { VentureTreasuryCard } from "../design1118/TreasuryLinks";
import { enterpriseAdapter } from "../design1118/enterpriseLive";
import { StartBody, toFormVenture, type Waiting } from "./EnterpriseStart";
import { VentureForm, MoneyForm, SavedLine } from "./EnterpriseForms";
import { ledgerWrites, type LedgerWrites } from "../business/ledgerWrite";
import { MarketNewsCards, ProposalsCard, TradingCard, WatchlistCard } from "../design1118/Trading";
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

function VentureCell({ v, w, onOpen, extra }: { v: Venture; w: Wired<Venture[]>; onOpen?: (id: string) => void; extra?: React.ReactNode }) {
  const h = HEALTH[v.health];
  return (
    <div className="cc-venture">
      <h3>{onOpen ? <button type="button" className="cc-venture__open" onClick={() => onOpen(v.id)}>{v.name}</button> : v.name} <StatusText kind={h.kind}>{h.text}</StatusText> {wiredTag(w)}</h3>
      <p>{[v.stage, v.progress ? `${v.progress.done} of ${v.progress.total} ${v.progress.label}` : null, v.note || null].filter(Boolean).join(" · ")}{v.target.state === "unwired" && v.target.reason === "currency" ? <> · target price <DataTag kind="currency" /></> : null}</p>
      {v.progress && <Meter size="sm" className="cc-meter--warn" label={`${v.name}: ${v.progress.done} of ${v.progress.total} ${v.progress.label}`} value={v.progress.done} max={v.progress.total} color={v.health === "at_risk" ? "var(--copper)" : undefined} />}
      {v.nextStep && <p>Next: {v.nextStep}</p>}
      {extra}
    </div>
  );
}

/** design-1111v (Taisun, 19:23 JST): Akiya is a personal item now and lives in Life, not in Ventures. */
export const PERSONAL_ITEMS = new Set(["akiya"]);
const enterpriseVentures = (w: Wired<Venture[]>): Wired<Venture[]> => (w.state === "unwired" ? w : { ...w, value: w.value.filter((v) => !PERSONAL_ITEMS.has(v.id)) });

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

/** `onOpen(itemId)` carries the one waiting item so Decisions opens on it; with several waiting it opens the page. */
export function DecisionsCard({ adapter, onOpen }: { adapter: BusinessAdapter; onOpen?: (itemId?: string) => void }) {
  const w = waitingDecisions(adapter);
  const line = w.unwired ? "Funding votes and tax flags from the ledger appear here." : w.n === 0 ? "Nothing is waiting for a decision." : [...w.open.map((v) => `${v.requester} ${v.purpose.replace(/ \(.*\)$/, "").toLowerCase()} vote`), ...w.tax.map((t) => `tax flag: ${t.label.charAt(0).toLowerCase()}${t.label.slice(1)}`)].join(" · ");
  return <LinkCard title={w.unwired ? "Decisions" : `Decisions · ${w.n} waiting`} line={line} open="Open Decisions ›" onOpen={onOpen ? () => onOpen(w.n === 1 ? (w.open[0]?.id ?? w.tax[0]?.id) : undefined) : undefined} tag={w.unwired ? <DataTag kind="wire" /> : w.sample ? <DataTag kind="sample" /> : null} />;
}

/** Enterprise › Overview, the start page (decision 0045 item 8): what this is, three actions, each venture's month, what needs your call.
 *  Trading appears once, as one line pointing at its full card in Ventures (it used to be a tile, a cell and a card). */
export function BusinessOverview({ data, adapter = businessAdapter(data), onOpenVenture, onView, work, writes = ledgerWrites() }: { data: SystemSnapshot; adapter?: BusinessAdapter; onOpenVenture: (id: string) => void; onView?: (view: string) => void; work: React.ReactNode; writes?: LedgerWrites | null }) {
  const { revenue } = data;
  const target = adapter.revenueTargetUsd();
  const ventures = enterpriseVentures(adapter.ventures());
  const w = waitingDecisions(adapter);
  const waiting: Waiting[] = [
    ...w.open.map((v) => ({ id: v.id, text: `Funding vote: ${v.purpose.charAt(0).toLowerCase()}${v.purpose.slice(1)}`, ventureName: v.requester, view: `decisions/${v.id}` })),
    ...w.tax.map((t) => ({ id: t.id, text: `Tax flag: ${t.label.charAt(0).toLowerCase()}${t.label.slice(1)}`, view: `decisions/${t.id}` })),
  ];
  const live = hasRevenue(revenue);
  const sampleTag = ventures.state === "sample" || w.sample;
  return (
    <div className="biz page-inset cc-page">
      <PageHero title="Enterprise" statement="What this is: the businesses you run — what each earned, what needs your call."
        detail={<>{live ? `${money(revenue.net_income)} net from trading in 30 days.` : "Add a venture, then log what it earns and costs."}{sampleTag ? <> <span className="cc-hero-tag"><DataTag kind="sample" /></span></> : null}</>} />
      <StartBody ventures={ventures.state === "unwired" ? [] : ventures.value} waiting={waiting} writes={writes} onOpenVenture={onOpenVenture} onView={onView}
        tradingLine={<p className="cc-note" data-testid="trading-line">Trading: {live ? `${money(revenue.month_revenue)} revenue in 30 days${target.state === "unwired" ? "" : `, target ${money(target.value)}`}. ` : "no revenue reported yet. "}<LinkButton small onClick={onView ? () => onView("ventures") : undefined}>Open Trading in Ventures ›</LinkButton></p>} />
      <AmaraTake page="business" className="biz__take" />
      {work}
    </div>
  );
}

/** Enterprise › Ventures (D-B5, D-B7, D-B8): every venture, or one on its own page (design-1111v: route ventures/<id>). */
/** Edit, log money for, and archive one venture. Archiving hides it; the ledger keeps it and Restore brings it back. */
function VentureTools({ v, writes, onChanged }: { v: Venture; writes: LedgerWrites | null; onChanged?: () => void }) {
  const [open, setOpen] = useState<"edit" | "money" | null>(null);
  const [note, setNote] = useState<{ text: string; undo: (() => Promise<boolean>) | null } | null>(null);
  const done = () => { setOpen(null); onChanged?.(); };
  const archive = async () => {
    if (!writes) { setNote({ text: "Archiving works in the Amara OS app.", undo: null }); return; }
    try { const c = await writes.archiveVenture(v.id, true); setNote({ text: `Archived ${v.name}. It stays in the ledger; Restore it from Archived ventures.`, undo: () => writes.undoChange(c.id) }); }
    catch { setNote({ text: "That didn't save. Nothing was changed.", undo: null }); }
  };
  return (
    <div className="v-composer">
      <div className="v-actions">
        <Button size="small" variant="secondary" onClick={() => setOpen(open === "edit" ? null : "edit")} aria-expanded={open === "edit"}>Edit {v.name}</Button>
        <Button size="small" variant="secondary" onClick={() => setOpen(open === "money" ? null : "money")} aria-expanded={open === "money"}>Log income or a cost</Button>
        <Button size="small" variant="plain" onClick={() => { void archive(); }}>Archive</Button>
      </div>
      {open === "edit" && <VentureForm writes={writes} venture={toFormVenture(v)} onClose={done} onSaved={onChanged} />}
      {open === "money" && <MoneyForm writes={writes} ventures={[toFormVenture(v)]} defaultVentureId={v.id} onClose={done} />}
      {note && <SavedLine text={note.text} undo={note.undo} />}
    </div>
  );
}

export function VenturesView({ data, adapter = businessAdapter(data), ventureId, onOpen, onView, onRoute, writes = ledgerWrites() }: { data: SystemSnapshot; adapter?: BusinessAdapter; ventureId: string | null; onOpen: (id: string | null) => void; onView?: (view: string) => void; onRoute?: (route: Route) => void; writes?: LedgerWrites | null }) {
  const [adding, setAdding] = useState(false);
  const [restored, setRestored] = useState<string | null>(null);
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
    const waitingVotes = waitingDecisions(adapter).open.filter((r) => r.requester === v.name);
    const waiting = waitingVotes.length;
    const nextStepChat = (label: string, more: string) => askMore({ from: `Enterprise · ${v.name}`, chips: [{ label: "Venture", value: v.name }, { label: "Next step", value: label }], question: `Help me with the next step for ${v.name}: ${label}${more ? ` (${more})` : ""}. What should I do first?`, sample: detail.state === "sample" });
    return (
      <div className="biz page-inset cc-page">
        <nav className="cc-crumb" aria-label="Breadcrumb"><Button className="cc-back" variant="plain" size="small" icon={ChevronLeft} onClick={() => onOpen(null)}>Ventures</Button> › {v.name}</nav>
        <PageHero title={`Venture · ${v.stage.toLowerCase()}`} statement={`${v.name} is ${h.text.toLowerCase()}`} detail={<>{d?.headline ?? (v.note || "No details yet. Use Edit to say what it is and what comes next.")}{d ? <> <span className="cc-hero-tag">{tag}</span></> : null}</>} />
        <VentureTools v={v} writes={writes} />
        {d && <ul className="cc-strip" aria-label="Venture summary">
          <li className="cc-tile"><span className="cc-tile__k">Tasks done</span><span className="cc-tile__v">{d ? `${d.checklist.filter((c) => c.done).length} of ${d.checklist.length}` : "\u2014"}</span><span className="cc-tile__s">{tag}</span></li>
          <li className="cc-tile"><span className="cc-tile__k">Budget used</span><span className="cc-tile__v">{d ? usd(d.budgetUsedUsd) : "\u2014"}</span><span className="cc-tile__s">{d ? wiredTag(d.budgetUsedUsd) : tag}</span></li>
          <li className="cc-tile"><span className="cc-tile__k">Fund balance</span><span className="cc-tile__v">{d ? usd(d.fundUsd) : "\u2014"}</span><span className="cc-tile__s">{d ? wiredTag(d.fundUsd) : tag}</span></li>
          <li className="cc-tile"><span className="cc-tile__k">Waiting decisions</span><span className="cc-tile__v">{waiting}</span><span className="cc-tile__s">{tag}</span></li>
        </ul>}
        {d && <>
          <div className="cc-grid2">
            <Card title="Launch tasks" accessory={acc(tag)}><ul className="cc-check">{d.checklist.map((c) => <li key={c.label} data-done={c.done || undefined}><span className="cc-sr">{c.done ? "Done: " : "To do: "}</span>{c.label}</li>)}</ul></Card>
            <Card title="Milestones" accessory={acc(tag)}>
              {d.milestones.map((m) => <div className="cc-row" key={m.label}><div className="cc-row__main"><span>{m.label}</span><small>{m.due}</small></div><StatusText kind={HEALTH[m.health].kind}>{HEALTH[m.health].text}</StatusText></div>)}
              {logged.map((r) => <div className="cc-row" key={r.id}><div className="cc-row__main"><span>Decided: {r.title.replace(/^[^·]+· /, "")}</span><small>{jst(r.decidedAt)} JST · from the decisions log{r.outcome ? ` · Outcome: ${r.outcome}` : ""}</small></div><StatusText kind="ok">{r.choice === "handled" ? "Handled" : r.choice === "yes" ? "Yes" : "No"}</StatusText></div>)}
            </Card>
          </div>
          <Card title="Next steps" accessory={acc(tag)}>{d.nextActions.map((a, i) => (
            <div className="cc-row" key={a.label}><div className="cc-row__main"><span>{a.label}</span><small>{a.detail}</small></div>
              {a.action === "Open vote" ? <LinkButton small onClick={onView ? () => onView(waitingVotes[0] ? `decisions/${waitingVotes[0].id}` : "decisions") : undefined}>Open in Decisions ›</LinkButton>
                : <Button size="small" variant={a.action === "Draft with crew" && i === 0 ? "primary" : "secondary"} onClick={() => nextStepChat(a.label, a.detail)}>{a.action}</Button>}
            </div>
          ))}</Card>
          <VentureGoalsCard ventureId={v.id} onOpen={onRoute} />
        </>}
        {(() => { const tv = enterpriseAdapter().ventures(); const tr = tv.state === "unwired" ? null : tv.value.find((x) => x.id === v.id); return tr ? <VentureTreasuryCard venture={tr} onOpen={onRoute} /> : null; })()}
      </div>
    );
  }
  const atRisk = list.filter((x) => x.health === "at_risk").length;
  const archived = (data.ledger?.ventures ?? []).filter((x) => x.archived);
  const restore = async (id: string, name: string) => {
    try { await writes?.archiveVenture(id, false); setRestored(writes ? `Restored ${name}.` : "Restoring works in the Amara OS app."); } catch { setRestored("That didn't save. Nothing was changed."); }
  };
  const riskDown = data.unavailableSources.some((s) => s.command === "arm_get_risk_status");
  return (
    <div className="biz page-inset cc-page d18-page">
      <PageHero title="Ventures" statement={ventures.state === "unwired" || list.length === 0 ? "Add your first venture" : `${list.length + 1} ventures${atRisk ? `, ${atRisk} at risk` : ""}`}
        detail={<>Each venture is a business you run. Trading is one of them: it comes from the revenue engine&apos;s live data, and its proposals are watch-only until you approve a record of your choice — never an order.{list.length ? <> {list.map((x) => x.name).join(" and ")} <span className="cc-hero-tag">{wiredTag(ventures)}</span></> : null}</>} />
      <div className="v-actions"><Button size="small" variant={adding ? "secondary" : "primary"} aria-expanded={adding} onClick={() => setAdding(!adding)}>Add a venture</Button></div>
      {adding && <Card title="Add a venture"><VentureForm writes={writes} onClose={() => setAdding(false)} /></Card>}
      <PageTake surface="ventures" className="biz__take" />
      <TradingCard data={data} />
      <MarketNewsCards />
      <ProposalsCard capUsd={riskDown ? undefined : data.risk.global_daily_loss_cap} />
      <WatchlistCard />
      <div className="d18-grid2 d18-grid2--end">
        {list.length === 0 ? (
          <Card title="Your ventures">
            <p className="cc-note">No ventures yet. Add one to track its stage, next step and what it earns.</p>
            <div className="v-actions"><Button size="small" variant="primary" onClick={() => setAdding(true)}>Add a venture</Button></div>
          </Card>
        ) : (
          <Card title="Your ventures" accessory={acc(wiredTag(ventures))}>{list.map((x) => <VentureCell key={x.id} v={x} w={ventures} onOpen={(id) => onOpen(id)} />)}</Card>
        )}
        {archived.length > 0 && (
          <Card title="Archived ventures">
            <InsetGroup aria-label="Archived ventures">{archived.map((a) => <ListRow key={a.id} title={a.name} subtitle="Hidden from your lists. Its history is kept." accessory={<Button size="small" variant="secondary" onClick={() => { void restore(a.id, a.name); }}>Restore</Button>} />)}</InsetGroup>
            {restored && <SavedLine text={restored} undo={null} />}
          </Card>
        )}
        <p className="cc-note">Akiya is a personal item now. Its dates are in {onRoute ? <LinkButton small onClick={() => onRoute({ id: "life", view: "dates" })}>Life › Dates &amp; deadlines</LinkButton> : "Life › Dates & deadlines"}.</p>
      </div>
    </div>
  );
}
