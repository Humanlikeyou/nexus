import { useState } from "react";
import { money } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Button, DataTag, Dialog, Sparkline, StatusText } from "../ui";
import { marketAdapter, type MarketProposal, type MarketProposalAdapter } from "./marketProposals";
import "./design1118.css";

/**
 * design-1118a Ventures › Trading (round 5). The trading ring and limits come from the revenue engine's
 * live risk status. Market, news and proposals are unwired until a feed exists (designed empty state);
 * sample previews use fictional tickers, always labelled. Approve opens a SECOND confirm sheet; Confirm
 * only records his choice. There is no order path anywhere in this file.
 */
export function TradingCard({ data }: { data: SystemSnapshot }) {
  const { risk } = data;
  const down = data.unavailableSources.some((s) => s.command === "arm_get_risk_status");
  const pct = risk.global_capital_limit > 0 ? Math.round((risk.global_capital_deployed / risk.global_capital_limit) * 100) : 0;
  const c = 2 * Math.PI * 27;
  return (
    <section className="d18-card" aria-labelledby="d18-trading">
      <div className="d18-map__head"><h2 id="d18-trading" className="d18-h">Trading</h2>
        {down ? <StatusText kind="off">Risk unavailable</StatusText> : risk.kill_switch_active ? <StatusText kind="fail">Kill switch engaged</StatusText> : <span className="d18-chip d18-chip--ok">Within limits</span>}</div>
      {down ? <p className="d18-note">The revenue engine&apos;s risk status couldn&apos;t be read.</p> : (
        <div className="d18-trade">
          <div className="d18-mring" data-tone="mint" style={{ width: 64, height: 64 }} role="img" aria-label={`${pct}% of capital deployed`}>
            <svg width="64" height="64" viewBox="0 0 64 64" aria-hidden="true"><circle className="d18-mring__track" cx="32" cy="32" r="27" /><circle className="d18-mring__fill" cx="32" cy="32" r="27" strokeDasharray={`${Math.max((pct / 100) * c, 0.01).toFixed(1)} ${c.toFixed(1)}`} transform="rotate(-90 32 32)" /></svg>
            <div className="d18-mring__v" aria-hidden="true"><b>{pct}%</b><small>deployed</small></div>
          </div>
          <ul className="d18-tiles" aria-label="Trading limits">
            <li><small>Daily P&amp;L</small><b>{money(risk.global_daily_pnl)} of {money(risk.global_daily_loss_cap)} cap</b></li>
            <li><small>Capital</small><b>{money(risk.global_capital_deployed)} of {money(risk.global_capital_limit)}</b></li>
            <li><small>Risk index</small><b>{risk.risk_index}</b></li>
            <li><small>Kill switch</small><b>{risk.kill_switch_active ? "On" : "Off"}</b></li>
            <li><small>Strategies</small><b>{risk.active_strategies} active</b></li>
          </ul>
        </div>
      )}
      {data.revenue.by_strategy.length > 0 && (
        <p className="d18-note">{data.revenue.by_strategy.map((b, i) => <span key={b.strategy_id}>{i ? " · " : ""}{b.strategy_name} {money(b.net)} net</span>)}</p>
      )}
    </section>
  );
}

const SIDE = { buy: "Buy", sell: "Sell", watch: "Watch" } as const;
const KIND = { crypto: "Crypto", stock: "Stock" } as const;

function useAdapterTick() {
  const [n, set] = useState(0);
  return [n, () => set((x) => x + 1)] as const;
}

function ProposalBody({ p, sample, capUsd }: { p: MarketProposal; sample: boolean; capUsd?: number }) {
  const risk = capUsd != null ? p.risk.replace("the daily loss cap", `the ${money(capUsd)} daily loss cap`) : p.risk;
  return (
    <>
      <p className="d18-prop__eyebrow"><b>{SIDE[p.side]} · {KIND[p.kind]}</b>{sample && <DataTag kind="sample" />}<span className="d18-chip d18-chip--quiet">Not financial advice</span></p>
      <h3 className="d18-prop__t">{p.title}</h3>
      <p className="d18-prop__why">{p.reason}</p>
      <p className="d18-note">Source · {p.source} · Confidence · {p.confidence[0].toUpperCase() + p.confidence.slice(1)}{sample ? " (sample)" : ""}</p>
      <p className="d18-note">Risk · {risk}</p>
    </>
  );
}

export function MarketNewsCards({ adapter: given }: { adapter?: MarketProposalAdapter }) {
  const adapter = given ?? marketAdapter();
  const market = adapter.market(), news = adapter.newsNote();
  return (
    <div className="d18-grid2">
      <section className="d18-card" aria-labelledby="d18-market">
        <div className="d18-map__head"><h2 id="d18-market" className="d18-h">Market</h2>{market.state === "sample" ? <DataTag kind="sample" /> : market.state === "unwired" ? <DataTag kind="wire" /> : null}</div>
        {market.state === "unwired" ? <p className="d18-note">A market feed isn&apos;t connected. No prices are shown until one is.</p> : (
          <>
            <div className="d18-market" aria-hidden="true"><Sparkline values={market.value} label={market.state === "sample" ? "Sample market line" : "Market"} size="lg" />{market.state === "sample" && <span className="d18-market__wm">Sample data</span>}</div>
            <p className="d18-note">{market.state === "sample" ? "Market feed is not connected. This sparkline is sample data, not a real price." : "From the market feed."}</p>
          </>
        )}
      </section>
      <section className="d18-card" aria-labelledby="d18-news">
        <div className="d18-map__head"><h2 id="d18-news" className="d18-h">News for proposals</h2>{news.state === "sample" ? <DataTag kind="sample" /> : news.state === "unwired" ? <DataTag kind="wire" /> : null}</div>
        <p className="d18-note">{news.state === "unwired" ? "Amara would gather crypto and stock ideas from world news here once a news feed is connected." : news.value}</p>
      </section>
    </div>
  );
}

export function ProposalsCard({ adapter: given, capUsd }: { adapter?: MarketProposalAdapter; capUsd?: number }) {
  const adapter = given ?? marketAdapter();
  const [, bump] = useAdapterTick();
  const proposals = adapter.proposals();
  const choices = adapter.choices();
  const [confirming, setConfirming] = useState<MarketProposal | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const sample = proposals.state === "sample";
  const recorded = new Map((choices.state === "unwired" ? [] : choices.value).map((c) => [c.proposalId, c]));
  const record = async (p: MarketProposal, choice: "approved" | "dismissed") => {
    setBusy(true);
    const r = await adapter.recordChoice(p.id, choice);
    setBusy(false); setConfirming(null);
    setNote(r.state === "unwired" ? "Your choice couldn't be saved: the choice log needs wiring. Nothing was sent." : choice === "approved" ? `Recorded: you approved “${p.title}”. No order was sent.` : `Dismissed “${p.title}”.`);
    bump();
  };
  return (
    <section className="d18-card" aria-labelledby="d18-props" data-testid="proposals">
      <div className="d18-map__head"><h2 id="d18-props" className="d18-h">Amara&apos;s proposed trades</h2><span className="d18-chip d18-chip--quiet">Not financial advice</span></div>
      <p className="d18-note">Proposals only. Approving records your choice. Amara never places a trade, and the ledger stays watch-only.</p>
      {proposals.state === "unwired" ? (
        <p className="d18-msg">No proposals yet. They appear once the news and market feeds are connected. <DataTag kind="wire" /></p>
      ) : proposals.value.length === 0 ? <p className="d18-msg">No open proposals.</p> : (
        <div className="d18-grid2">
          {proposals.value.map((p) => {
            const done = recorded.get(p.id);
            return (
              <article className="d18-prop" key={p.id} data-state={done?.choice ?? "open"}>
                <ProposalBody p={p} sample={sample} capUsd={capUsd} />
                {done?.choice === "approved" ? <p className="d18-msg" role="status">Recorded as approved. No order was sent.</p> : (
                  <>
                    <div className="d18-actions"><Button size="small" variant="primary" onClick={() => setConfirming(p)}>Approve</Button><Button size="small" variant="secondary" onClick={() => void record(p, "dismissed")}>Dismiss</Button></div>
                    <p className="d18-note">Approve only records your choice. No order is placed, and no keys are used.</p>
                  </>
                )}
              </article>
            );
          })}
        </div>
      )}
      {note && <p className="d18-msg" role="status">{note}</p>}
      <Dialog open={confirming != null} title="Confirm this record" confirmLabel="Confirm" busy={busy} onConfirm={() => { if (confirming) void record(confirming, "approved"); }} onCancel={() => setConfirming(null)} className="d18-confirm">
        {confirming && (
          <div data-testid="trade-confirm">
            <ProposalBody p={confirming} sample={sample} capUsd={capUsd} />
            <p>This only records your choice. No order is sent, and no keys are used.</p>
            <p>Not financial advice.</p>
          </div>
        )}
      </Dialog>
    </section>
  );
}
