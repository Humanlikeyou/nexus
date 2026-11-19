import { useEffect, useRef, useState } from "react";
import { money } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Button, DataTag, Dialog, Sparkline, StatusText } from "../ui";
import { marketAdapter, type MarketProposal, type MarketProposalAdapter } from "./marketProposals";
import type { LiveMarket } from "./marketProposalsLive";
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
  const [undo, setUndo] = useState<string | null>(null); // History id, live only
  const [toWatch, setToWatch] = useState(false);
  useEffect(() => { if (!undo) return; const t = window.setTimeout(() => setUndo(null), 8000); return () => window.clearTimeout(t); }, [undo]);
  const sample = proposals.state === "sample";
  const recentW = typeof (adapter as Partial<LiveMarket>).recent === "function" ? (adapter as LiveMarket).recent() : null;
  const recent = recentW && recentW.state === "live" ? recentW.value : null;
  const recorded = new Map((choices.state === "unwired" ? [] : choices.value).map((c) => [c.proposalId, c]));
  const record = async (p: MarketProposal, choice: "approved" | "dismissed") => {
    setBusy(true);
    const r = await adapter.recordChoice(p.id, choice);
    setBusy(false); setConfirming(null);
    const toast = r.state === "live" ? r.value.toast : undefined;
    const base = toast ?? (choice === "approved" ? `Recorded: you approved “${p.title}”. No order was sent.` : `Dismissed “${p.title}”.`);
    setNote(r.state === "unwired" ? "Your choice couldn't be saved: the choice log needs wiring. Nothing was sent." : choice === "approved" ? `${base} Now on your watchlist.` : base);
    setToWatch(r.state === "live" && choice === "approved");
    setUndo(r.state === "live" && r.value.historyId ? r.value.historyId : null);
    bump();
  };
  const undoChoice = async () => {
    const id = undo; setUndo(null);
    const ok = id && typeof (adapter as Partial<LiveMarket>).undoWatch === "function" ? await (adapter as LiveMarket).undoWatch(id) : false;
    setNote(ok ? "Undone. The proposal is open again." : "That couldn't be undone. Check History.");
    setToWatch(false);
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
      {note && <p className="d18-msg" role="status">{note}{toWatch && <> <Button size="small" variant="plain" onClick={() => document.getElementById("d18-watch")?.scrollIntoView?.({ behavior: "smooth" })}>See watchlist</Button></>}{undo && <> <Button size="small" variant="plain" onClick={() => { void undoChoice(); }}>Undo</Button></>}</p>}
      {recent && recent.length > 0 && (
        <div data-testid="recently-decided">
          <p className="d18-eyebrow">Recently decided</p>
          <ul className="d18-clist">
            {recent.map((d) => <li key={d.id}><div><b>{d.title}</b><small>{d.choice === "approved" ? "Recorded as approved. On your watchlist. No order was sent." : "Dismissed."} · {d.at.slice(0, 10)}</small></div></li>)}
          </ul>
          <p className="d18-note">Every choice is also in History, where you can undo it for 8 seconds.</p>
        </div>
      )}
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

const RESEARCH_NOTE: Record<string, string> = {
  started: "Amara is looking for new picks. They arrive as proposals above.",
  already_running: "Amara is already researching. New picks arrive as proposals above.",
  blocked: "Research is blocked while outbound access is off. Turn it on in Settings to research.",
};
const RESEARCH_POLL_MS = 10_000;
const RESEARCH_MAX_POLLS = 30;

/** A pick's source as an outbound link: https only, opened by the user's click (the shell plugin opens _blank links). Nothing is fetched here. */
export function SourceLink({ name, url }: { name: string; url: string | null }) {
  let host = "";
  try {
    const u = url != null ? new URL(url) : null;
    // https only, and never a URL with credentials (`https://good.com@evil.com/` shows good.com but goes to evil.com).
    if (u && u.protocol === "https:" && !u.username && !u.password) host = u.hostname;
  } catch { host = ""; }
  return host ? <><a href={url as string} target="_blank" rel="noopener noreferrer" aria-label={`Open source: ${name}`}>{name}</a> ({host})</> : <>{name}</>;
}
const STATUS_LABEL = { open: "Market open", closed: "Market closed", stale: "Price is stale" } as const;

/** Watchlist (decision 0038). Read-only prices: no price is ever invented ("No price yet" when none was fetched). */
export function WatchlistCard({ adapter: given }: { adapter?: MarketProposalAdapter }) {
  const adapter = (given ?? marketAdapter()) as Partial<LiveMarket> & MarketProposalAdapter;
  const [, bump] = useAdapterTick();
  const [note, setNote] = useState<string | null>(null);
  const [undo, setUndo] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [watching, setWatching] = useState(false);
  const [, poke] = useState(0);
  const base = useRef<{ finished: string | null; sawRunning: boolean; polls: number }>({ finished: null, sawRunning: false, polls: 0 });
  useEffect(() => { if (!undo) return; const t = window.setTimeout(() => setUndo(null), 8000); return () => window.clearTimeout(t); }, [undo]);
  const w = typeof adapter.watchlist === "function" ? adapter.watchlist() : null;
  const research_ = w && w.state === "live" ? w.value.research : null;
  // Poll (bounded) while research runs, then say what it found.
  useEffect(() => {
    if (!watching || typeof adapter.refresh !== "function") return;
    const id = window.setInterval(() => {
      base.current.polls += 1;
      if (base.current.polls > RESEARCH_MAX_POLLS) { setWatching(false); setNote("Research is taking a while. New picks will still arrive as proposals."); return; }
      void adapter.refresh!().then(() => poke((n) => n + 1));
    }, RESEARCH_POLL_MS);
    return () => window.clearInterval(id);
  }, [watching, adapter]);
  useEffect(() => {
    if (!watching || !research_) return;
    if (research_.running) { base.current.sawRunning = true; return; }
    if (base.current.sawRunning || research_.lastFinishedAt !== base.current.finished) {
      setWatching(false);
      const n = research_.lastProposed;
      setNote(n > 0 ? `Amara found ${n} new pick${n === 1 ? "" : "s"}. They are in the proposals above.` : "No new picks.");
    }
  }, [watching, research_]);
  if (!w) return null;
  const research = async () => {
    setBusy(true);
    const r = await adapter.researchNow!();
    setBusy(false);
    if (r.state === "unwired") setNote("Research couldn't start. Try again in a moment.");
    else if (r.value.state === "rate_limited") setNote(`Research ran recently. Try again in ${r.value.retryAfterMinutes ?? "a few"} minutes.`);
    else {
      setNote(RESEARCH_NOTE[r.value.state] ?? null);
      if (r.value.state === "started" || r.value.state === "already_running") {
        base.current = { finished: w.state === "live" ? w.value.research.lastFinishedAt : null, sawRunning: r.value.state === "already_running", polls: 0 };
        setWatching(true);
      }
    }
    bump();
  };
  const drop = async (ticker: string) => {
    const r = await adapter.dropPick!(ticker);
    if (r.state === "unwired") { setNote("That didn't save. Try again in a moment."); return; }
    setNote(`Removed ${ticker} from the watchlist.`); setUndo(r.value.historyId); bump();
  };
  const undoDrop = async () => { const id = undo; setUndo(null); const ok = id ? await adapter.undoWatch!(id) : false; setNote(ok ? "Undone. The pick is back." : "That couldn't be undone. Check History."); bump(); };
  return (
    <section className="d18-card" aria-labelledby="d18-watch" data-testid="watchlist">
      <div className="d18-map__head"><h2 id="d18-watch" className="d18-h">Watchlist</h2><span className="d18-chip d18-chip--quiet">Not financial advice</span>
        {w.state === "unwired" ? <DataTag kind="wire" /> : <Button size="small" variant="secondary" disabled={busy || w.value.research.running} onClick={() => { void research(); }}>Research now</Button>}</div>
      {w.state === "unwired" ? <p className="d18-note">The watchlist isn&apos;t available yet.</p> : w.value.picks.length === 0 ? <p className="d18-note">No picks yet. Research now asks Amara for ideas. Watch-only: nothing is ever bought.</p> : (
        <ul className="d18-clist">
          {w.value.picks.map((p) => (
            <li key={p.ticker}><div><b>{p.ticker}</b><small>{p.reason}{p.source ? <> · <SourceLink name={p.source} url={p.sourceUrl} /></> : null}</small>
              <small>{p.quote.price == null ? "No price yet" : `${p.quote.price}${p.quote.currency ? ` ${p.quote.currency}` : ""}`} · {STATUS_LABEL[p.quote.status]}</small></div>
              <Button size="small" variant="plain" aria-label={`Drop ${p.ticker}`} onClick={() => { void drop(p.ticker); }}>Drop</Button></li>
          ))}
        </ul>
      )}
      {note && <p className="d18-msg" role="status">{note}{undo && <> <Button size="small" variant="plain" onClick={() => { void undoDrop(); }}>Undo</Button></>}</p>}
    </section>
  );
}
