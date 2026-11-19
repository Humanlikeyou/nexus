import { useState } from "react";
import { Button, Card, DataTag, SegmentedControl, wiredTag } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { AmaraTakeCard, type TakeCardSlots } from "../amaraTake/AmaraTakeCard";
import { PREVIEW_DEPS, previewMode, sampleTakeView, takeView, type TakeView } from "../amaraTake/takeView";
import { useAmaraTakeStatus } from "../amaraTake/useAmaraTake";
import { MemoryLineView } from "../takes/TakeExtras";
import { SuggestionBlock } from "../takes/SuggestionBlock";
import { Nums, Rich } from "../takes/parts";
import { moneyAdapter, NFA_LINE, type MoneyAccount, type MoneyAdapter, type Region } from "./adapters";
import { ConnectSheet } from "./ConnectSheet";
import "../takes/takes.css";
import "./money.css";

export type Currency = "USD" | "JPY";
const usd = (v: number) => v.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
const jpy = (v: number) => `${Math.round(v).toLocaleString("en-US")} JPY`;
export const shown = (v: number, cur: Currency, rate: number) => (cur === "USD" ? usd(v) : jpy(v * rate));

const REGION: Record<Region, string> = { us: "United States · USD", jp: "Japan · JPY", crypto: "Crypto · Ledger" };
const STATUS_WORD: Record<MoneyAccount["status"], string> = { ok: "", stale: "Out of date", expired: "Expired", error: "Error", not_connected: "Not connected" };

function AccountRow({ a, adapter }: { a: MoneyAccount; adapter: MoneyAdapter }) {
  const [note, setNote] = useState<string | null>(null);
  const greyed = a.status === "expired" || a.status === "stale" || a.status === "not_connected";
  const badge = a.route === "watch_only" ? <span className="m-badge" data-k="watch"><span aria-hidden="true">👁</span> Watch-only</span> : <span className="m-badge"><span aria-hidden="true">🔒</span> Read-only</span>;
  return (
    <div className="m-acct" data-status={a.status} data-greyed={greyed || undefined}>
      <span className="m-mono" aria-hidden="true">{a.institution === "jp_bank" ? "銀" : a.institution === "ledger" ? (a.currency === "ETH" ? "Ξ" : "₿") : a.nickname[0]}</span>
      <div className="m-acct__main"><span>{a.nickname}</span><small>{a.via}</small>{a.note && <small className="m-acct__note">{a.note}</small>}</div>
      <div className="m-acct__badges">{badge}<span className="m-badge m-badge--quiet">Can&apos;t move money</span></div>
      <span className="m-acct__sync" data-status={a.status}>{a.status === "ok" ? <><span aria-hidden="true">{a.route === "csv" ? "◔" : "✓"}</span> {a.synced}</> : <><span aria-hidden="true">!</span> {STATUS_WORD[a.status]}{a.status !== "not_connected" ? ` · ${a.synced}` : ""}</>}</span>
      <span className="m-acct__amt"><b>{a.native}</b>{a.asOf ? <small>as of {a.asOf}</small> : a.currency !== "USD" && (a.usd != null ? <small>≈ {usd(a.usd)}</small> : a.currency === "BTC" || a.currency === "ETH" || a.native === "–" ? null : <small>USD <DataTag kind="wire" /></small>)}</span>
      {(a.status === "expired" || a.status === "stale") && (
        <div className="m-acct__acts">
          {a.status === "expired" ? <><Button size="small" variant="primary" onClick={() => { void adapter.reconnect(a.id).then((r) => setNote(r.state === "unwired" ? "Signing in again needs wiring." : "Sample data: opens Chime's own sign-in page in your browser.")); }}>Sign in again…</Button>
            <Button size="small" onClick={() => { void adapter.disconnect(a.id).then((r) => setNote(r.state === "unwired" ? "Needs wiring, so nothing changed." : "Sample data: disconnected in the preview only.")); }}>Disconnect</Button></>
            : <Button size="small" onClick={() => { void adapter.importStatement().then((r) => setNote(r.state === "sample" ? "Sample data: no file is read in the preview." : adapter.connectNote() ?? "Statement import needs wiring.")); }}>Import a statement…</Button>}
          {note && <span className="pk-note" role="status">{note}</span>}
        </div>
      )}
    </div>
  );
}

/** m1 + m6: connected accounts with read-only / watch-only badges, plus what Amara can and can't do. */
export function MoneyAccounts({ adapter = moneyAdapter() }: { adapter?: MoneyAdapter }) {
  const [open, setOpen] = useState(false);
  const w = adapter.accounts();
  const list = valueOf(w);
  const connect = <Button size="small" variant="primary" onClick={() => setOpen(true)}>Connect an account…</Button>;
  return (
    <>
      <div className="m-callout" role="note"><b><span aria-hidden="true">🔒</span> Read-only by design.</b> Each connection only has permission to read. There&apos;s no transfer, payment, trade or withdrawal permission anywhere in Amara, and no button for one. Sign-in details and access tokens stay in this Mac&apos;s Keychain, never in backups, logs or chat.</div>
      <Card title="Connected accounts" accessory={<span className="pk-acc">{wiredTag(w)}{connect}</span>}>
        {!list ? <p className="pk-note">Account connections <DataTag kind="wire" />. Chime and Fidelity need a read-only data service, Japanese banks a statement import and Ledger a watch-only address. No balances are shown and no money suggestions are made until one is connected.</p>
          : list.length === 0 ? <div className="t-empty"><b>Connect your accounts to see personal money</b><span>Nothing is connected. Amara shows only what you type into Personal (bills, subscriptions) until you connect a read-only account.</span></div>
          : (["us", "jp", "crypto"] as Region[]).map((r) => {
            const rows = list.filter((a) => a.region === r);
            return rows.length === 0 ? null : <div key={r} className="m-group"><h3 className="m-region">{REGION[r]}</h3>{rows.map((a) => <AccountRow key={a.id} a={a} adapter={adapter} />)}</div>;
          })}
        <p className="pk-note">Account numbers are never shown. Amara stores a nickname and the last sync time; the connection itself lives in the Keychain (shown only as Set or Not set). Japanese bank amounts stay in yen, with the USD equivalent beside them.</p>
      </Card>
      <div className="m-split">
        <Card title="What Amara can and can't do">
          <div className="pk-row"><span className="m-yes" aria-hidden="true">✓</span><div className="pk-row__main"><span>Read balances, transactions and holdings</span><small>To show your totals and spot fees, idle cash and savings rate</small></div></div>
          <div className="pk-row"><span className="m-yes" aria-hidden="true">✓</span><div className="pk-row__main"><span>Suggest one thing a day, with its reasons</span><small>You do it in your own bank or Fidelity app</small></div></div>
          <div className="pk-row"><span className="m-no" aria-hidden="true">–</span><div className="pk-row__main"><span>Move, pay, trade, convert or withdraw</span><small>Not possible: no connection has that permission</small></div></div>
          <div className="pk-row"><span className="m-no" aria-hidden="true">–</span><div className="pk-row__main"><span>See your Ledger recovery phrase or private keys</span><small>Never asked for, and refused if pasted</small></div></div>
        </Card>
        <Card title="Ledger safety">
          <div className="m-warn" role="note"><b>Amara will never ask for your 24 words.</b>Watch-only uses public addresses or an xpub, which can see balances but can&apos;t spend. If anything, including a message that looks like Amara, asks for your recovery phrase, it&apos;s a scam.</div>
        </Card>
      </div>
      <ConnectSheet open={open} onClose={() => setOpen(false)} adapter={adapter} />
    </>
  );
}

/** m3: personal total, tiles, allocation, native + USD, the "FX moved it" split; venture funds kept out. USD with a JPY view. */
export function MoneyOverviewSection({ adapter = moneyAdapter(), currency, onCurrency }: { adapter?: MoneyAdapter; currency: Currency; onCurrency: (c: Currency) => void }) {
  const w = adapter.overview();
  const v = valueOf(w);
  if (!v) return null;
  // JPY view needs a stored rate; without one the figure is "Needs wiring" rather than a guess.
  const s = (n: number) => (currency === "JPY" && !v.fx ? <DataTag kind="currency" /> : shown(n, currency, v.fx?.rate ?? 1));
  const usdOrWire = (n: number | null) => (n == null ? <DataTag kind="wire" /> : s(n));
  const tag = w.state === "sample" ? <DataTag kind="sample" /> : null;
  return (
    <>
      <div className="m-bar"><SegmentedControl label="Show amounts in" size="small" value={currency} options={[{ value: "USD", label: "USD" }, { value: "JPY", label: "JPY" }]} onChange={(c) => onCurrency(c as Currency)} />
        <span className="pk-note">{v.fx ? <>{currency === "USD" ? "Yen converted" : "Dollars converted"} at {v.fx.rate.toFixed(2)} JPY = 1 USD ({v.fx.at}, {v.fx.source}) {tag}</> : <>No FX rate stored, so yen has no USD value <DataTag kind="wire" /></>}</span></div>
      <ul className="cc-strip" aria-label="Personal money">
        <li className="cc-tile"><span className="cc-tile__k">Cash</span><span className="cc-tile__v">{usdOrWire(v.cash.usd)}</span><span className="cc-tile__s">{v.cash.note}</span></li>
        <li className="cc-tile"><span className="cc-tile__k">Investments</span><span className="cc-tile__v">{usdOrWire(v.investments.usd)}</span><span className="cc-tile__s">{v.investments.note}</span></li>
        <li className="cc-tile"><span className="cc-tile__k">Crypto (watch-only)</span><span className="cc-tile__v">{usdOrWire(v.crypto.usd)}</span><span className="cc-tile__s">{v.crypto.note}</span></li>
        <li className="cc-tile"><span className="cc-tile__k">Emergency buffer</span><span className="cc-tile__v">{v.bufferMonths == null ? <DataTag kind="wire" /> : `${v.bufferMonths} months`}</span><span className="cc-tile__s">{v.bufferNote}</span></li>
      </ul>
      <div className="m-split">
        <Card title="Where it sits" accessory={<span className="pk-acc">{tag}</span>}>
          {v.allocation.length > 0 && <><div className="m-alloc" role="img" aria-label={v.allocation.map((a) => `${a.label} ${a.pct}%`).join(", ")}>{v.allocation.map((a) => <i key={a.label} data-t={a.tone} style={{ flexGrow: a.pct }} />)}</div>
          <div className="t-legend">{v.allocation.map((a) => <span key={a.label}><i className="m-sw" data-t={a.tone} />{a.label} {a.pct}%</span>)}</div></>}
          <table className="pk-table" aria-label="Balances by account">
            <thead><tr><th scope="col">Account</th><th scope="col" className="r">Native</th><th scope="col" className="r">{currency}</th><th scope="col" className="r">30 days</th></tr></thead>
            <tbody>{v.rows.map((r) => <tr key={r.name}><td>{r.name}</td><td className="r">{r.native}</td><td className="r">{usdOrWire(r.usd)}</td><td className="r">{r.d30}{r.d30note && <small>{r.d30note}</small>}</td></tr>)}</tbody>
          </table>
          <p className="pk-note"><b>JPY note:</b> yen stays yen in your banks. The USD figures use one daily reference rate, so a USD total can move when you didn&apos;t do anything. The &ldquo;FX moved it&rdquo; line separates that from what you saved.</p>
        </Card>
        <div className="pk-stack">
          {v.venture.length > 0 && <Card title="Venture funds · not in your personal total" accessory={<span className="pk-acc">{tag}</span>}>
            {v.venture.map((x) => <div className="pk-row" key={x.name}><div className="pk-row__main"><span>{x.name}</span><small>{x.sub}</small></div><span className="pk-num">{usdOrWire(x.usd)}</span></div>)}
            <p className="pk-note">Kept apart so a good venture month never looks like personal savings, and a venture loss never hides inside them. Moving money between them is your call, in your bank.</p>
          </Card>}
          <Card title="Savings rate" accessory={<span className="pk-acc">{v.savingsRate ? tag : null}</span>}>
            {v.savingsRate ? <>
              <p className="m-big">{v.savingsRate.pct}% <small>of income, {v.savingsRate.month}</small></p>
              <p className="pk-delta" data-d="up">{v.savingsRate.delta}</p>
              <p className="pk-note">{v.savingsRate.note}</p></>
              : <p className="pk-note">Needs income and spending data, which aren&apos;t wired yet. <DataTag kind="wire" /></p>}
          </Card>
        </div>
      </div>
    </>
  );
}

/** m4 / m6 #6: the money take on Business › Personal. Business Overview keeps its venture take. */
export function MoneyTake({ adapter = moneyAdapter() }: { adapter?: MoneyAdapter }) {
  const w = adapter.take();
  const t = valueOf(w);
  if (!t) {
    const view: TakeView = { state: "unwired", reason: "The money take needs at least one connected account" };
    return <div className="take-slot m-take"><AmaraTakeCard page="business" view={view} /></div>;
  }
  const view: TakeView = { state: "fresh", note: t.note, sample: w.state === "sample" };
  const block = t.kind === "partial"
    ? <div className="t-empty"><b>No money suggestion today.</b><span>{t.why}</span></div>
    : <><SuggestionBlock s={t.suggestion} detects eyebrow="Today's one thing · Money" footnote="Amara judges this by what you do (saving, fees), never by next month's market."
        onAnswer={(st, r) => adapter.answer(t.suggestion.id, st, r)} onUndo={() => adapter.undo(t.suggestion.id)} />
      <p className="m-nfa" role="note">{NFA_LINE}</p></>;
  return <div className="take-slot m-take"><AmaraTakeCard page="business" view={view} slots={{ memory: t.kind === "suggestion" && t.memory ? <MemoryLineView text={t.memory} /> : null, history: <span className="take__hist take__hist--static">History · {t.historyCount} takes</span>, block, refresh: "refreshes once after the next sync" }} /></div>;
}

/** Life › Personal money (1111w fix): one "Amara's take". The 1111t money insight and
 *  recommendation live inside the Life take card. MoneyAdapter.take() is unchanged. */
function MoneyInsight({ adapter }: { adapter: MoneyAdapter }) {
  const w = adapter.take();
  const t = valueOf(w);
  if (!t) {
    return (
      <div className="life-money">
        <p className="life-money__k">Money</p>
        <p className="take__quiet">The money take needs at least one connected account. It appears once a read-only account is connected and the money adapter answers. Nothing here is written by Amara today. <DataTag kind="wire" /></p>
      </div>
    );
  }
  const rec = t.kind === "partial"
    ? <div className="t-empty"><b>No money suggestion today.</b><span>{t.why}</span></div>
    : <><SuggestionBlock s={t.suggestion} detects eyebrow="Today's one thing · Money" footnote="Amara judges this by what you do (saving, fees), never by next month's market."
        onAnswer={(st, r) => adapter.answer(t.suggestion.id, st, r)} onUndo={() => adapter.undo(t.suggestion.id)} />
      <p className="m-nfa" role="note">{NFA_LINE}</p></>;
  return (
    <div className="life-money">
      <p className="life-money__k">Money</p>
      <p className="take__body"><Rich text={t.note.text} /></p>
      {t.note.chips.length > 0 && (
        <ul className="take__chips" aria-label="Numbers Amara used for money">
          {t.note.chips.map((c) => <li key={`${c.label}-${c.value}`} className="take__chip"><span>{c.label}</span>{c.value}</li>)}
        </ul>
      )}
      {t.kind === "suggestion" && t.memory ? <MemoryLineView text={t.memory} /> : null}
      {rec}
    </div>
  );
}

export function PersonalMoneyTake({ adapter = moneyAdapter() }: { adapter?: MoneyAdapter }) {
  const preview = previewMode();
  const status = useAmaraTakeStatus("life", preview ? PREVIEW_DEPS : undefined);
  const lifeView = preview ? sampleTakeView("life") : takeView("life", status.value, status.checking);
  const w = adapter.take();
  const t = valueOf(w);
  // If Life isn't a fresh note, AmaraTakeCard won't render slots. When money has data, use the
  // money note as the card body (still page="life") so the insight isn't dropped. When both are
  // unwired, the Life unwired card is the single Needs wiring state.
  const view: TakeView = lifeView.state === "fresh" || !t
    ? lifeView
    : { state: "fresh", note: t.note, sample: w.state === "sample" };
  const history = t ? <span className="take__hist take__hist--static">History · {t.historyCount} takes</span> : undefined;
  const slots: TakeCardSlots = lifeView.state === "fresh" || !t
    ? { history, block: <MoneyInsight adapter={adapter} />, refresh: t ? "refreshes once after the next sync" : undefined }
    : {
        memory: t.kind === "suggestion" && t.memory ? <MemoryLineView text={t.memory} /> : null,
        history,
        block: t.kind === "partial"
          ? <div className="t-empty"><b>No money suggestion today.</b><span>{t.why}</span></div>
          : <><SuggestionBlock s={t.suggestion} detects eyebrow="Today's one thing · Money" footnote="Amara judges this by what you do (saving, fees), never by next month's market."
              onAnswer={(st, r) => adapter.answer(t.suggestion.id, st, r)} onUndo={() => adapter.undo(t.suggestion.id)} />
            <p className="m-nfa" role="note">{NFA_LINE}</p></>,
        refresh: "refreshes once after the next sync",
      };
  return <div className="take-slot life__take m-take"><AmaraTakeCard page="life" view={view} slots={slots} /></div>;
}

/** m5: money follow-ups grade only what you control. Market returns are shown, never judged; FX is "can't tell" by design. */
export function MoneyFollowUps({ adapter = moneyAdapter() }: { adapter?: MoneyAdapter }) {
  const w = adapter.followUps();
  const list = valueOf(w);
  if (!list) return null;
  return (
    <Card title="Money follow-ups · Amara grades what you control, not the market" accessory={<span className="pk-acc"><DataTag kind="sample" /></span>}>
      <div className="t-board">
        {list.map((f) => (
          <div key={f.id} className="t-mini">
            <span className="m-group__k">{f.group}</span>
            <span className="t-verdict" data-v={f.verdict === "locked_in" || f.verdict === "better" ? "better" : "unclear"}>{f.verdict === "locked_in" || f.verdict === "better" ? "▲" : "?"} {f.chip}</span>
            <h4>{f.title}</h4>
            <p><Rich text={f.body} /></p>
            <Nums rows={f.rows} />
          </div>
        ))}
      </div>
      <p className="t-rules"><b>Money rules.</b> Suggestions are conservative and come from a reviewed list: buffer first (3–6 months), then high-interest debt, then low fees, broad diversification and automatic saving. No single stocks, no crypto top-ups, no leverage, no market timing, no tips. Every money take ends with &ldquo;Not financial advice&rdquo;, and Amara can&apos;t move money.</p>
    </Card>
  );
}
