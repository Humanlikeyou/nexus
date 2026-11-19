import { useEffect, useState } from "react";
import { Button, Card, DataTag, PageHero, StatusText, TextField, wiredTag } from "../ui";
import { valueOf, type Wired } from "../lib/sampleAdapters";
import { MEMORY_LABEL, PERSONAL_DEFAULTS, type Memory, type MemoryCategory, type PersonalAdapter } from "./adapters";
import { Bar, Confidence, PairBars, Rich, Scatter, money } from "./parts";
import { FollowThroughView } from "../takes/FollowThrough";
import { takesAdapter } from "../takes/adapters";
import { TakesSettingsCard } from "../takes/TakesSettings";
import { moneyAdapter } from "../money/adapters";
import { MoneyAccounts, MoneyFollowUps, MoneyOverviewSection, PersonalMoneyTake, shown, type Currency } from "../money/MoneyView";
import "../ui/PanelKit.css";
import "./personal.css";
import "../adds/v1111.css";

const WIRE_NOTE = "Needs wiring: this appears once Claude builds its store (HANDOFF-1111a). Nothing here is made up.";

/** Mind–Body › Patterns (1111a #2): findings as sentences with the numbers, strength and confidence behind them. */
export function PatternsView({ adapter, initialTab = "findings" }: { adapter: PersonalAdapter; initialTab?: "findings" | "follow" }) {
  const pw = adapter.patterns();
  const v = valueOf(pw);
  const [top, ...rest] = v?.findings ?? [];
  const [tab, setTab] = useState<"findings" | "follow">(initialTab);
  const outw = takesAdapter().outcomes();
  const outcomes = valueOf(outw);
  const chips = (
    <div className="pk-chips" role="group" aria-label="Show">
      <button type="button" className="pk-chip ci-chip" aria-pressed={tab === "findings"} data-on={tab === "findings" || undefined} onClick={() => setTab("findings")}>Findings</button>
      <button type="button" className="pk-chip ci-chip" aria-pressed={tab === "follow"} data-on={tab === "follow" || undefined} onClick={() => setTab("follow")}>Follow-through{outcomes ? ` ${outcomes.length}` : ""}</button>
    </div>
  );
  if (tab === "follow") {
    return (
      <div className="mind-body page-inset pk-host">
        <PageHero title="Patterns" statement="Follow-through: what Amara suggested and what changed" detail={outcomes ? <>{outcomes.length} suggestion types · grouped by type over time · every result shows its numbers {outw.state === "sample" && <DataTag kind="sample" />}</> : <>Grouped by suggestion type over time <DataTag kind="wire" /></>} />
        {chips}
        <FollowThroughView />
      </div>
    );
  }
  return (
    <div className="mind-body page-inset pk-host">
      <PageHero title="Patterns" statement={v ? v.headline : "What moves your focus and calm"} detail={v ? <>{v.window} {pw.state === "sample" && <DataTag kind="sample" />}</> : <>Recomputed every Sunday from Fitness, Neuro and check-ins <DataTag kind="wire" /></>} />
      {chips}
      {!v ? <Card title="Findings" accessory={<span className="pk-acc"><DataTag kind="wire" /></span>}><p className="pk-note">No findings yet. Amara needs at least 3 days with and without each factor before it says anything. {WIRE_NOTE}</p></Card> : (
        <>
          {top && (
            <Card title={top.title} accessory={<span className="pk-acc">{pw.state === "sample" && <DataTag kind="sample" />}</span>}>
              <div className="pk-finding">
                <div><Confidence n={top.confidence} label={`${top.strength} · ${top.n}`} /><h3><Rich text={top.sentence} /></h3><p><Rich text={top.detail} /></p>
                  {top.chips && <div className="pk-chips">{top.chips.map((c, i) => <span key={c} className="pk-chip" data-on={i === 0 || undefined}>{c}</span>)}</div>}</div>
                {top.chart.kind === "scatter" && <div><Scatter {...top.chart} /><div className="pk-legend"><span><i className="c" />under {top.chart.split} h</span><span><i />{top.chart.split} h+</span><span>x: sleep · y: focus</span></div></div>}
              </div>
            </Card>
          )}
          <div className="pk-g2">
            {rest.map((f) => (
              <Card key={f.id} title={f.title} accessory={<span className="pk-acc">{pw.state === "sample" && <DataTag kind="sample" />}</span>}>
                <div className="pk-finding pk-finding--one"><div><Confidence n={f.confidence} label={`${f.strength} · ${f.n}`} /><h3><Rich text={f.sentence} /></h3><p><Rich text={f.detail} /></p></div>
                  {f.chart.kind === "bars" && <><PairBars rows={f.chart.rows} legend={f.chart.legend} /><div className="pk-legend"><span><i className="c" />{f.chart.legend[0]}</span><span><i />{f.chart.legend[1]}</span></div></>}</div>
              </Card>
            ))}
          </div>
        </>
      )}
      <p className="pk-note">How this works: Amara compares days you already track (Fitness, Neuro, check-ins), shows the numbers behind every sentence and how many days back it. A pattern, not proof. Strength: r from 0 to 1. Confidence grows with more days. Never medical advice.</p>
    </div>
  );
}

const amt = (w: Wired<number>) => (w.state === "unwired" ? wiredTag(w) : money(w.value));

/** Life › Personal money (1111a #4 + 1111t money; moved from Business › Personal in design-1111v, contract unchanged):
 *  read-only accounts, balances in USD with a JPY view, one Life take card with the money insight and recommendation folded in, follow-ups, then bills and subscriptions.
 *  Personal only: venture funds never appear here; Enterprise money is in Enterprise › Finances. */
export function PersonalMoneyView({ adapter, onOpenFinances }: { adapter: PersonalAdapter; onOpenFinances?: () => void }) {
  const pm = adapter.money();
  const v = valueOf(pm);
  const ma = moneyAdapter();
  const ovw = ma.overview();
  const acctsw = ma.accounts();
  const ov = valueOf(ovw);
  const accts = valueOf(acctsw);
  const [cur, setCur] = useState<Currency>("USD");
  // design-1111v: the "Kept apart" banner and the "Business › Personal" eyebrow are gone; a link points to Finances.
  const finances = <> · Enterprise money is in {onOpenFinances ? <button type="button" className="v-link" onClick={onOpenFinances}>Enterprise › Finances ›</button> : "Enterprise › Finances"}</>;
  const statement = ov ? (ov.totalUsd != null ? `${shown(ov.totalUsd, ov.fx ? cur : "USD", ov.fx?.rate ?? 1)} personal, across ${ov.balances} balances` : "Balances entered, no personal total yet") : accts && accts.length ? `${accts.filter((a) => a.status !== "not_connected").length} accounts connected, all read-only` : "Your own accounts, read-only";
  const detail = ov ? <>Personal only · {ov.fx ? cur : "USD"}{ov.fx ? <> · yen converted at ¥{ov.fx.rate.toFixed(2)} = $1</> : " · no FX rate stored"}{finances} {ovw.state === "sample" && <DataTag kind="sample" />}</>
    : accts ? <>Personal only · Amara can see balances and transactions; she can never move money{finances} {acctsw.state === "sample" && <DataTag kind="sample" />}</>
    : <>Personal only · read-only connections and statement import{finances} <DataTag kind="wire" /></>;
  return (
    <div className="biz page-inset cc-page pk-host">
      <PageHero title="Personal money" statement={statement} detail={detail} />
      <PersonalMoneyTake adapter={ma} />
      <MoneyOverviewSection adapter={ma} currency={cur} onCurrency={setCur} />
      <MoneyAccounts adapter={ma} />
      <MoneyFollowUps adapter={ma} />
      {!v ? <Card title="Bills, subscriptions and spending" accessory={<span className="pk-acc"><DataTag kind="wire" /></span>}><p className="pk-note">No bills or subscriptions yet. Nothing is shown until you pick the accounts; no balances or bills are guessed. {WIRE_NOTE}</p></Card> : (
        <>
          <ul className="cc-strip" aria-label="Personal money this month">
            <li className="cc-tile"><span className="cc-tile__k">Spent in {v.month}</span><span className="cc-tile__v">{amt(v.spent)}</span><span className="cc-tile__s">{v.spentNote}</span></li>
            <li className="cc-tile"><span className="cc-tile__k">Subscriptions</span><span className="cc-tile__v">{amt(v.subsMonthly)} / mo</span><span className="cc-tile__s">{v.subsNote}</span></li>
            <li className="cc-tile"><span className="cc-tile__k">Bills due · 14 days</span><span className="cc-tile__v">{amt(v.billsDue)}</span><span className="cc-tile__s">{v.billsNote}</span></li>
            <li className="cc-tile"><span className="cc-tile__k">Left after bills</span><span className="cc-tile__v">{amt(v.leftAfterBills)}</span><span className="cc-tile__s">{v.payday}</span></li>
          </ul>
          <div className="pk-g2">
            <Card title="Bills due · next 14 days" accessory={<span className="pk-acc">{pm.state === "sample" && <DataTag kind="sample" />}</span>}>
              <table className="pk-table" aria-label="Bills due"><thead><tr><th scope="col">Bill</th><th scope="col">Due</th><th scope="col" className="r">Amount</th><th scope="col">Status</th></tr></thead>
                <tbody>{v.bills.map((b) => <tr key={b.id}><td>{b.name}<small>{b.sub}</small></td><td>{b.due}</td><td className="r">{amt(b.amount)}</td><td>{b.status === "pay" ? <StatusText kind="wait">Pay</StatusText> : <StatusText kind="ok">{b.status === "auto" ? "Auto" : "Scheduled"}</StatusText>}</td></tr>)}</tbody></table>
            </Card>
            <Card title={`Subscriptions · ${amt(v.subsMonthly)} / mo`} accessory={<span className="pk-acc">{pm.state === "sample" && <DataTag kind="sample" />}</span>}>
              {v.subscriptions.map((s) => <div className="pk-row" key={s.id}><div className="pk-row__main"><span>{s.name}</span><small>{s.sub}</small></div>{s.unusedDays != null && <span className="pk-pill" data-tone="warn">Unused {s.unusedDays} d</span>}<span className="pk-num">{amt(s.amount)}</span></div>)}
              <p className="pk-note">{v.moreSubs}</p>
            </Card>
          </div>
          <Card title={`Spending by category · ${v.month}`} accessory={<span className="pk-acc">{pm.state === "sample" && <DataTag kind="sample" />}</span>}>
            <div className="pk-g2">
              <div>{v.categories.map((c) => <div className="pk-cat" key={c.name}><span>{c.name}</span><Bar pct={c.pct} copper={c.up} /><span>{money(c.amount)}</span></div>)}</div>
              <p className="pk-take"><Rich text={v.summary} /></p>
            </div>
          </Card>
        </>
      )}
    </div>
  );
}

/** `undoId` is set after a live edit or delete: the block whose earlier text `restoreMemory` can put back (8 s window). */
function MemoryRow({ m, adapter, onDone }: { m: Memory; adapter: PersonalAdapter; onDone: (note: string, undoId?: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(m.text);
  const [confirm, setConfirm] = useState(false);
  const result = (r: Wired<{ ok: true }>, what: string) => onDone(adapter.memoryWriteNote?.() ?? (r.state === "unwired" ? `${what} needs wiring, so nothing changed.` : r.state === "sample" ? `Sample data: ${what.toLowerCase()} in the preview only.` : `${what} done.`), r.state === "live" && adapter.restoreMemory ? m.id : undefined);
  if (editing) {
    return (
      <form className="pk-mem" onSubmit={(e) => { e.preventDefault(); void adapter.editMemory(m.id, text.trim()).then((r) => { setEditing(false); result(r, "Editing"); }); }}>
        <TextField label="Memory" hideLabel value={text} maxLength={2000} onChange={(e) => setText(e.target.value)} />
        <div className="pk-mem__acts"><Button size="small" onClick={() => { setEditing(false); setText(m.text); }}>Cancel</Button><Button size="small" variant="primary" type="submit" disabled={!text.trim()}>Save</Button></div>
      </form>
    );
  }
  return (
    <div className="pk-mem">
      <div>{m.text}<small>{m.source}</small></div>
      <div className="pk-mem__acts">
        {confirm ? <><Button size="small" onClick={() => setConfirm(false)}>Keep</Button><Button size="small" variant="destructive" onClick={() => { void adapter.deleteMemory(m.id).then((r) => { setConfirm(false); result(r, "Deleting"); }); }}>Delete for good</Button></>
          : <><Button size="small" aria-label={`Edit: ${m.text}`} onClick={() => setEditing(true)}>Edit</Button><Button size="small" variant="destructive" aria-label={`Delete: ${m.text}`} onClick={() => setConfirm(true)}>Delete</Button></>}
      </div>
    </div>
  );
}

/** Settings › Memory (1111a #5): what Amara remembers, grouped and searchable; edit, delete and confirm new ones. */
export function MemorySection({ adapter }: { adapter: PersonalAdapter }) {
  const [, bump] = useState(0);
  const [q, setQ] = useState("");
  const [cat, setCat] = useState<MemoryCategory | "all">("all");
  const [note, setNote] = useState<string | null>(null);
  const [undoId, setUndoId] = useState<string | null>(null);
  const mem = adapter.memories();
  const pend = adapter.pendingMemories();
  const all = valueOf(mem);
  const pending = valueOf(pend) ?? [];
  const note0 = adapter.memoriesNote?.() ?? null;
  const done = (n: string, undo?: string) => { setNote(n); setUndoId(undo ?? null); bump((x) => x + 1); };
  // The standard 8 s Undo window (same as Smart home): after that the button goes and the note stays.
  useEffect(() => { if (!undoId) return; const t = window.setTimeout(() => setUndoId(null), 8000); return () => window.clearTimeout(t); }, [undoId]);
  const undo = () => { const id = undoId; if (!id || !adapter.restoreMemory) return; setUndoId(null); void adapter.restoreMemory(id).then((r) => done(adapter.memoryWriteNote?.() ?? (r.state === "unwired" ? "Undo didn't go through. Nothing changed." : "Put back the earlier text."))); };
  const shown = (all ?? []).filter((m) => (cat === "all" || m.category === cat) && m.text.toLowerCase().includes(q.trim().toLowerCase()));
  const cats = (Object.keys(MEMORY_LABEL) as MemoryCategory[]).filter((c) => (all ?? []).some((m) => m.category === c));
  return (
    <div className="pk-host memory">
      <PageHero title="Memory" statement="What Amara knows about you" detail={all ? (mem.state === "live" ? <>{all.length} memory {all.length === 1 ? "block" : "blocks"} · stored on this Mac (Letta) · edits and deletes change Amara&apos;s memory here, and the old text is saved first</> : <>{all.length} memories · stored on this Mac (Letta) · you can edit or delete any of them {mem.state === "sample" && <DataTag kind="sample" />}</>) : <>Stored on this Mac (Letta) · editing and deleting need wiring <DataTag kind="wire" /></>} />
      {!all ? <Card title="Memories" accessory={<span className="pk-acc">{!note0 && <DataTag kind="wire" />}</span>}><p className="pk-note" role={note0 ? "status" : undefined}>{note0 ?? "Amara's memory list appears here once the app can read it from Letta. Nothing is read or changed in Letta until then."}</p></Card> : (
        <>
          <div className="pk-search memory__search">
            <label htmlFor="memory-q" className="ui-visually-hidden">Search memories</label>
            <input id="memory-q" type="search" className="memory__q" placeholder="Search memories" value={q} onChange={(e) => setQ(e.target.value)} />
            <span className="pk-chips" role="group" aria-label="Category">
              {(["all", ...cats] as const).map((c) => <button key={c} type="button" className="pk-chip ci-chip" aria-pressed={cat === c} data-on={cat === c || undefined} onClick={() => setCat(c)}>{c === "all" ? `All ${all.length}` : `${MEMORY_LABEL[c]} ${all.filter((m) => m.category === c).length}`}</button>)}
            </span>
          </div>
          {pending.length > 0 && (
            <Card title={`Amara wants to remember · ${pending.length}`} accessory={<span className="pk-acc">{wiredTag(pend)}</span>}>
              {pending.map((p) => (
                <div className="pk-row" key={p.id}><StatusText kind="wait" /><div className="pk-row__main"><span>&ldquo;{p.text}&rdquo;</span><small>{p.source}</small></div>
                  <Button size="small" onClick={() => { void adapter.decideMemory(p.id, false).then((r) => done(r.state === "unwired" ? "Needs wiring." : "Not kept.")); }}>Don&apos;t keep</Button>
                  <Button size="small" variant="primary" onClick={() => { void adapter.decideMemory(p.id, true).then((r) => done(r.state === "unwired" ? "Needs wiring." : r.state === "sample" ? "Sample data: kept in the preview only." : "Kept.")); }}>Keep</Button></div>
              ))}
              <p className="pk-note">New {PERSONAL_DEFAULTS.confirmMemories.map((c) => MEMORY_LABEL[c].toLowerCase()).join(" and ")} memories, and anything Amara infers rather than hears from you, wait here until you keep them.</p>
            </Card>
          )}
          <div className="pk-g2 memory__grid">
            {(cat === "all" ? cats : [cat]).map((c) => {
              const rows = shown.filter((m) => m.category === c);
              return rows.length === 0 ? null : (
                <Card key={c} title={MEMORY_LABEL[c]} accessory={<span className="pk-acc">{wiredTag(mem)}</span>}>
                  {rows.map((m) => <MemoryRow key={`${m.id}-${m.text}`} m={m} adapter={adapter} onDone={done} />)}
                </Card>
              );
            })}
          </div>
          {shown.length === 0 && <p className="pk-note">No memories match.</p>}
          {note && <p className="pk-note" role="status">{note}{undoId && <> <Button size="small" variant="plain" onClick={undo}>Undo</Button></>}</p>}
        </>
      )}
      <TakesSettingsCard />
      <div className="pk-info">{mem.state === "live" ? <>Deleting empties the memory in Amara&apos;s store (Letta) on this Mac and asks you first. The block itself stays, and the old text is saved on this Mac so it can be put back (Undo shows for 8 seconds after a change). Amara&apos;s own &ldquo;wants to remember&rdquo; suggestions aren&apos;t connected yet. Past chat transcripts are separate and stay until you delete them in Agents.</> : <>Deleting removes the memory from Amara&apos;s store (Letta) on this Mac and from future answers; it can&apos;t be undone. Past chat transcripts are separate and stay until you delete them in Agents.</>}</div>
    </div>
  );
}
