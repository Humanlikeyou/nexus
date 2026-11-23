import { useCallback, useEffect, useMemo, useState } from "react";
import { Button, Card, DataTag, Dialog, Select, SegmentedControl, TextField } from "../ui";
import { CATEGORIES, CATEGORY_LABEL, KIND_LABEL, draftFrom, entryError, formatMinor, makeMoneyEntries, type Category, type Currency, type EntryKind, type MoneyEntries, type Recurrence, type WireCurrencyView, type WireEntry, type WireMonthView } from "./entries";

const KINDS: EntryKind[] = ["balance", "income", "expense", "savings", "bill", "commitment"];
const todayJst = () => new Intl.DateTimeFormat("sv-SE", { timeZone: "Asia/Tokyo" }).format(new Date());

/** The Add sheet: one form, six kinds. Every field is validated before anything is sent. */
export function AddEntrySheet({ open, onClose, onSaved, api, today = todayJst }: { open: boolean; onClose: () => void; onSaved: (e: WireEntry) => void; api: MoneyEntries; today?: () => string }) {
  const [kind, setKind] = useState<EntryKind>("balance");
  const [currency, setCurrency] = useState<Currency>("JPY");
  const [amount, setAmount] = useState("");
  const [date, setDate] = useState(today());
  const [due, setDue] = useState("");
  const [recurrence, setRecurrence] = useState<Recurrence>("none");
  const [category, setCategory] = useState<Category | "">("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);
  const bill = kind === "bill" || kind === "commitment";
  const save = async () => {
    const r = draftFrom({ kind, amount, currency, date, due, recurrence, category, note }, today());
    if (!r.ok) { setError(r.error); return; }
    try { const e = await api.add(r.draft); setError(null); setAmount(""); setNote(""); onSaved(e); onClose(); } catch (e) { setError(entryError(e)); }
  };
  return (
    <Dialog open={open} title="Add to your money" onCancel={onClose} className="m-sheet">
      <SegmentedControl label="What are you adding" size="small" value={kind} options={KINDS.map((k) => ({ value: k, label: KIND_LABEL[k] }))} onChange={(k) => { setKind(k as EntryKind); setError(null); }} />
      <SegmentedControl label="Currency" size="small" value={currency} options={[{ value: "JPY", label: "JPY" }, { value: "USD", label: "USD" }]} onChange={(c) => setCurrency(c as Currency)} />
      <TextField label={`Amount (${currency})`} inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} />
      <TextField label={kind === "balance" ? "As of" : "Date"} value={date} onChange={(e) => setDate(e.target.value)} placeholder="YYYY-MM-DD" />
      {kind === "expense" && <Select label="Category" value={category} onChange={(e) => setCategory(e.target.value as Category | "")} options={[{ value: "", label: "Not set" }, ...CATEGORIES.map((c) => ({ value: c, label: CATEGORY_LABEL[c] }))]} />}
      {bill && <>
        <TextField label="Due date" value={due} onChange={(e) => setDue(e.target.value)} placeholder="YYYY-MM-DD" />
        <Select label="Repeats" value={recurrence} onChange={(e) => setRecurrence(e.target.value as Recurrence)} options={[{ value: "none", label: "Once" }, { value: "monthly", label: "Monthly" }, { value: "weekly", label: "Weekly" }, { value: "yearly", label: "Yearly" }]} />
      </>}
      <TextField label="Note (optional)" value={note} maxLength={200} onChange={(e) => setNote(e.target.value)} />
      {error && <p className="pk-note" role="alert">{error}</p>}
      <div className="pk-sheet__actions"><Button variant="secondary" onClick={onClose}>Cancel</Button><Button variant="primary" onClick={() => { void save(); }}>Save</Button></div>
    </Dialog>
  );
}

/** The dashboard numbers for one currency. Missing values are "–", never 0. */
export function dashboardTiles(v: WireCurrencyView) {
  const m = v.months[v.months.length - 1];
  const has = !!m?.has_data;
  const c = v.currency;
  return [
    { k: "Available now", v: formatMinor(v.available?.amount_minor, c), s: v.available ? `Balance of ${v.available.base_date} plus entries since` : "Add the money you have now" },
    { k: "This month income", v: has ? formatMinor(m.income_minor, c) : "–", s: has ? m.month : "Nothing recorded yet" },
    { k: "This month spent", v: has ? formatMinor(m.expense_minor, c) : "–", s: has ? m.month : "Nothing recorded yet" },
    { k: "Saved this month", v: has ? formatMinor(m.savings_minor, c) : "–", s: has ? m.month : "Nothing recorded yet" },
    { k: "Savings goal", v: v.goal ? `${v.goal.pct}%` : "–", s: v.goal ? `${formatMinor(v.goal.saved_minor, c)} of ${formatMinor(v.goal.target_minor, c)}` : "No savings goal set" },
    { k: "Upcoming bills", v: v.upcoming.length ? formatMinor(v.upcoming.reduce((a, u) => a + u.amount_minor, 0), c) : "–", s: v.upcoming.length ? `${v.upcoming.length} in the next 30 days` : "None in the next 30 days" },
  ];
}

export function MoneyEntriesPanel({ api = makeMoneyEntries() }: { api?: MoneyEntries }) {
  const [view, setView] = useState<WireMonthView | null | "failed">(null);
  const [recent, setRecent] = useState<WireEntry[]>([]);
  const [open, setOpen] = useState(false);
  const [cur, setCur] = useState<Currency>("JPY");
  const [note, setNote] = useState<string | null>(null);
  const load = useCallback(async () => {
    try { const [v, l] = await Promise.all([api.monthView(6), api.list()]); setView(v); setRecent(l.entries.filter((e) => !e.voided_by).slice(0, 8)); } catch { setView("failed"); }
  }, [api]);
  // eslint-disable-next-line react-hooks/set-state-in-effect -- loads an external store once per mount (and shows the cached page first)
  useEffect(() => { void load(); }, [load]);
  const cv = useMemo(() => (view && view !== "failed" ? view.currencies.find((c) => c.currency === cur) ?? view.currencies[0] ?? null : null), [view, cur]);
  const tiles = cv ? dashboardTiles(cv) : null;
  const undo = async (id: string) => { try { await api.undo(id); setNote("Undone. The entry stays in your history."); } catch (e) { setNote(entryError(e)); } await load(); };
  return (
    <Card title="Your money, by hand" accessory={<span className="pk-acc"><Button size="small" variant="primary" onClick={() => setOpen(true)}>Add…</Button></span>}>
      {view === "failed" && <p className="pk-note">Entries <DataTag kind="wire" />. The money store didn&apos;t answer, so nothing is shown.</p>}
      {view && view !== "failed" && view.currencies.length > 1 && <SegmentedControl label="Currency" size="small" value={cv?.currency ?? cur} options={view.currencies.map((c) => ({ value: c.currency, label: c.currency }))} onChange={(c) => setCur(c as Currency)} />}
      <ul className="cc-strip" aria-label="Personal money dashboard">
        {(tiles ?? dashboardTiles({ currency: cur, months: [], available: null, upcoming: [], commitments: [], committed_monthly_minor: 0, goal: null, savings_series: [], balance_series: [], has_statement: false, statement_through: null, excluded_duplicates: 0 })).map((t) => <li key={t.k} className="cc-tile"><span className="cc-tile__k">{t.k}</span><span className="cc-tile__v">{t.v}</span><span className="cc-tile__s">{t.s}</span></li>)}
      </ul>
      {view && view !== "failed" && view.needs_conversion && <p className="pk-note">JPY and USD are shown separately: no exchange rate with a date is stored.</p>}
      {recent.length > 0 && <div aria-label="Recent entries">{recent.map((e) => <div className="pk-row" key={e.id}><div className="pk-row__main"><span>{KIND_LABEL[e.kind]} · {formatMinor(e.amount_minor, e.currency)}</span><small>{e.date}{e.note ? ` · ${e.note}` : ""}</small></div><Button size="small" onClick={() => { void undo(e.id); }}>Undo</Button></div>)}</div>}
      {note && <p className="pk-note" role="status">{note}</p>}
      <AddEntrySheet open={open} onClose={() => setOpen(false)} onSaved={() => { setNote(null); void load(); }} api={api} />
    </Card>
  );
}
