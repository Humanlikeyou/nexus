import { useEffect, useState } from "react";
import { Button, SegmentedControl, Select, TextField } from "../ui";
import { amountText, jstDay, parseMinor, STAGES, writeError, type Currency, type LedgerWrites } from "../business/ledgerWrite";
import "../adds/v1111.css";

/**
 * Enterprise entry forms (decision 0045 item 8): add or edit a venture, log income or a cost, set the yen rate.
 * Each save says what happened in a role="status" line with an 8 s Undo. Nothing here moves money.
 */
export interface FormVenture { id: string; name: string; stage?: string; note?: string; nextStep?: string | null; currency?: Currency; targetMinor?: number | null }

const NEEDS_APP = "Saving works in the Amara OS app. This preview can't write files.";

/** A status line with an Undo that disappears after 8 s. */
export function SavedLine({ text, undo }: { text: string; undo: (() => Promise<boolean>) | null }) {
  const [live, setLive] = useState(true);
  const [msg, setMsg] = useState(text);
  useEffect(() => {
    const id = window.setTimeout(() => setLive(false), 8000);
    return () => window.clearTimeout(id);
  }, []);
  const run = () => {
    if (!undo) return;
    setLive(false);
    void undo().then((ok) => setMsg(ok ? "Undone." : "That can't be undone any more. Edit it instead.")).catch(() => setMsg("That couldn't be undone."));
  };
  return <p className="v-note" role="status">{msg}{undo && live && <> <Button size="small" variant="plain" onClick={run}>Undo</Button></>}</p>;
}

const minorField = (minor: number | null | undefined, c: Currency) => (minor == null ? "" : c === "JPY" ? String(minor) : (minor / 100).toFixed(2));

export function VentureForm({ writes, venture, onClose, onSaved }: { writes: LedgerWrites | null; venture?: FormVenture; onClose: () => void; onSaved?: (id: string) => void }) {
  const editing = Boolean(venture);
  const [name, setName] = useState(venture?.name ?? "");
  const [stage, setStage] = useState(venture?.stage && (STAGES as readonly string[]).includes(venture.stage) ? venture.stage : venture?.stage ?? "Idea");
  const [oneLiner, setOneLiner] = useState(venture?.note ?? "");
  const [nextStep, setNextStep] = useState(venture?.nextStep ?? "");
  const [currency, setCurrency] = useState<Currency>(venture?.currency ?? "JPY");
  const [target, setTarget] = useState(minorField(venture?.targetMinor, venture?.currency ?? "JPY"));
  const [errors, setErrors] = useState<{ name?: string; target?: string; form?: string }>({});
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<{ text: string; undo: (() => Promise<boolean>) | null } | null>(null);
  const stages = (STAGES as readonly string[]).includes(stage) ? [...STAGES] : [stage, ...STAGES];
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const next: typeof errors = {};
    if (!name.trim()) next.name = "Give the venture a name.";
    const minor = target.trim() ? parseMinor(target, currency) : null;
    if (target.trim() && minor === null) next.target = currency === "JPY" ? "Enter whole yen, like 300000." : "Enter dollars, like 1500 or 1500.50.";
    if (!writes) next.form = NEEDS_APP;
    setErrors(next);
    if (Object.keys(next).length || !writes) return;
    setBusy(true);
    try {
      const c = await writes.saveVenture({ id: venture?.id ?? null, name, stage, oneLiner, nextStep: nextStep.trim() || null, monthlyTargetMinor: minor, currency });
      setSaved({ text: `${editing ? "Saved" : "Added"} ${name.trim()}.`, undo: () => writes.undoChange(c.id) });
      onSaved?.(c.subject);
    } catch (err) { setErrors({ form: writeError(err) }); } finally { setBusy(false); }
  };
  return (
    <form className="v-composer" onSubmit={(e) => { void submit(e); }} aria-label={editing ? "Edit venture" : "Add a venture"} noValidate>
      <TextField label="Name" value={name} maxLength={60} onChange={(e) => setName(e.target.value)} error={errors.name} placeholder="Taiyo Skin" />
      <Select label="Stage" value={stage} onChange={(e) => setStage(e.target.value)} options={stages.map((s) => ({ value: s, label: s }))} description="Where it is today." />
      <TextField label="What it is" value={oneLiner} maxLength={140} onChange={(e) => setOneLiner(e.target.value)} placeholder="Skin care for travellers" description="One line, for future you." />
      <TextField label="Next step" value={nextStep} maxLength={140} onChange={(e) => setNextStep(e.target.value)} placeholder="Choose a supplier" />
      <SegmentedControl label="Currency" size="small" value={currency} onChange={(c) => { setCurrency(c); setTarget(""); }} options={[{ value: "JPY", label: "JPY" }, { value: "USD", label: "USD" }]} />
      <TextField label="Monthly revenue target (optional)" inputMode="decimal" value={target} onChange={(e) => setTarget(e.target.value)} error={errors.target} description={`In ${currency}. Leave empty if you don't have one.`} />
      {errors.form && <p className="v-note" role="alert">{errors.form}</p>}
      <div className="v-actions">
        <Button type="submit" size="small" variant="primary" disabled={busy}>{editing ? "Save changes" : "Add venture"}</Button>
        <Button type="button" size="small" variant="plain" onClick={onClose}>{saved ? "Close" : "Cancel"}</Button>
      </div>
      {saved && <SavedLine text={saved.text} undo={saved.undo} />}
    </form>
  );
}

export function MoneyForm({ writes, ventures, defaultVentureId, onClose, onAddVenture }: { writes: LedgerWrites | null; ventures: FormVenture[]; defaultVentureId?: string; onClose: () => void; onAddVenture?: () => void }) {
  const first = defaultVentureId && ventures.some((v) => v.id === defaultVentureId) ? defaultVentureId : ventures[0]?.id ?? "";
  const [ventureId, setVentureId] = useState(first);
  const [kind, setKind] = useState<"income" | "cost">("income");
  const [currency, setCurrency] = useState<Currency>(ventures.find((v) => v.id === first)?.currency ?? "JPY");
  const [amount, setAmount] = useState("");
  const [date, setDate] = useState(jstDay());
  const [note, setNote] = useState("");
  const [errors, setErrors] = useState<{ amount?: string; date?: string; form?: string }>({});
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<{ text: string; undo: () => Promise<boolean> } | null>(null);
  if (ventures.length === 0) {
    return (
      <div className="v-composer">
        <p className="cc-note">Money is logged against a venture. Add your first venture, then come back here.</p>
        <div className="v-actions">{onAddVenture && <Button size="small" variant="primary" onClick={onAddVenture}>Add a venture</Button>}<Button size="small" variant="plain" onClick={onClose}>Close</Button></div>
      </div>
    );
  }
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const next: typeof errors = {};
    const minor = parseMinor(amount, currency);
    if (minor === null || minor <= 0) next.amount = currency === "JPY" ? "Enter a whole number of yen above 0." : "Enter an amount above 0, like 25 or 25.50.";
    if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) next.date = "Pick a date.";
    if (!writes) next.form = NEEDS_APP;
    setErrors(next);
    if (Object.keys(next).length || !writes || minor === null) return;
    setBusy(true);
    try {
      const r = await writes.addEntry({ ventureId, date, amountMinor: minor, currency, kind, note: note.trim() || null });
      const who = ventures.find((v) => v.id === ventureId)?.name ?? "the venture";
      setSaved({ text: `Logged ${kind === "income" ? "income" : "a cost"} of ${amountText(minor, currency)} for ${who} on ${date}.`, undo: () => writes.undoEntry(r.id) });
      setAmount(""); setNote("");
    } catch (err) { setErrors({ form: writeError(err) }); } finally { setBusy(false); }
  };
  return (
    <form className="v-composer" onSubmit={(e) => { void submit(e); }} aria-label="Log income or a cost" noValidate>
      <Select label="Venture" value={ventureId} onChange={(e) => { setVentureId(e.target.value); const c = ventures.find((v) => v.id === e.target.value)?.currency; if (c) setCurrency(c); }} options={ventures.map((v) => ({ value: v.id, label: v.name }))} />
      <SegmentedControl label="Money in or out" size="small" value={kind} onChange={setKind} options={[{ value: "income", label: "Income" }, { value: "cost", label: "A cost" }]} />
      <SegmentedControl label="Currency" size="small" value={currency} onChange={setCurrency} options={[{ value: "JPY", label: "JPY" }, { value: "USD", label: "USD" }]} />
      <TextField label="Amount" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} error={errors.amount} description="Just the number. A cost is saved as money out." />
      <TextField label="Date" type="date" value={date} onChange={(e) => setDate(e.target.value)} error={errors.date} />
      <TextField label="Note (optional)" value={note} maxLength={80} onChange={(e) => setNote(e.target.value)} placeholder="Packaging, first sale…" />
      {errors.form && <p className="v-note" role="alert">{errors.form}</p>}
      <p className="v-note">This only records a number. Nothing is paid or sent.</p>
      <div className="v-actions">
        <Button type="submit" size="small" variant="primary" disabled={busy}>{kind === "income" ? "Log income" : "Log cost"}</Button>
        <Button type="button" size="small" variant="plain" onClick={onClose}>{saved ? "Done" : "Cancel"}</Button>
      </div>
      {saved && <SavedLine key={saved.text + saved.undo} text={saved.text} undo={saved.undo} />}
    </form>
  );
}

export function RateForm({ writes, current }: { writes: LedgerWrites | null; current?: { usdPerJpy: number; asOf: string } | null }) {
  const [rate, setRate] = useState(current ? String(Math.round((1 / current.usdPerJpy) * 100) / 100) : "");
  const [asOf, setAsOf] = useState(jstDay());
  const [err, setErr] = useState<string | undefined>();
  const [saved, setSaved] = useState<{ text: string; undo: () => Promise<boolean> } | null>(null);
  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    const n = Number(rate);
    if (!rate.trim() || !Number.isFinite(n) || n < 20 || n > 1000) { setErr("Enter yen per one US dollar, like 150.25."); return; }
    if (!writes) { setErr(NEEDS_APP); return; }
    setErr(undefined);
    try {
      const c = await writes.setRate(n, asOf);
      setSaved({ text: `Saved: 1 USD = ${n} JPY as of ${asOf}.`, undo: () => writes.undoChange(c.id) });
    } catch (x) { setErr(writeError(x)); }
  };
  return (
    <form className="v-composer" onSubmit={(e) => { void submit(e); }} aria-label="Exchange rate" noValidate>
      <TextField label="Yen per 1 US dollar" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} error={err} description="You enter it by hand; Amara never looks one up. Dollar figures only appear with a rate and its date." />
      <TextField label="Rate as of" type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} />
      <div className="v-actions"><Button type="submit" size="small" variant="primary">Save rate</Button></div>
      {saved && <SavedLine key={saved.text} text={saved.text} undo={saved.undo} />}
    </form>
  );
}
