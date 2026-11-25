import { useState } from "react";
import { Button, TextField } from "../ui";
import { isDay, type MoneyEntries } from "./entries";

export type StoredFx = { rate: number; date: string; source: string };
export type FxCheck = { ok: true; rate: number; date: string } | { ok: false; field: "rate" | "date"; error: string };

/** JPY per 1 USD: a positive number below 1000 (the backend's own limit), and a real date that is not in the future. */
export function validateFx(rateText: string, dateText: string, today: string): FxCheck {
  const t = rateText.trim().replace(/,/g, "");
  const rate = /^\d+(\.\d{1,4})?$/.test(t) ? Number(t) : NaN;
  if (!Number.isFinite(rate) || rate <= 0 || rate >= 1000) return { ok: false, field: "rate", error: "Enter JPY per 1 USD as a number above 0 and below 1,000, for example 151.20." };
  const date = dateText.trim();
  if (!isDay(date)) return { ok: false, field: "date", error: "Enter the rate's date as YYYY-MM-DD." };
  if (date > today) return { ok: false, field: "date", error: "That date is in the future. Enter the day the rate was published." };
  return { ok: true, rate, date };
}

const SOURCE: Record<string, string> = { ecb: "European Central Bank", manual: "typed by you" };
const errorText = (e: unknown) => (e === "invalid_input" ? "That rate isn't valid. Nothing was saved." : "The rate couldn't be saved. Nothing changed.");

/** Manual FX rate: shows the stored rate with its date and source; saving replaces it. Undo restores the previous rate only when you had typed it. */
export function FxRateForm({ api, fx, today, onChanged }: { api: Pick<MoneyEntries, "setRate">; fx: StoredFx | null; today: string; onChanged: () => void }) {
  const [rate, setRate] = useState("");
  const [date, setDate] = useState(today);
  const [errs, setErrs] = useState<{ rate?: string; date?: string }>({});
  const [msg, setMsg] = useState<string | null>(null);
  const [prev, setPrev] = useState<StoredFx | null>(null);
  const save = async () => {
    const r = validateFx(rate, date, today);
    if (!r.ok) { setErrs({ [r.field]: r.error }); setMsg(null); return; }
    setErrs({});
    try {
      await api.setRate(r.rate, r.date);
      setPrev(fx && fx.source === "manual" ? fx : null);
      setMsg(`Saved: ${r.rate} JPY per 1 USD as of ${r.date}.`); setRate(""); onChanged();
    } catch (e) { setMsg(errorText(e)); }
  };
  const undo = async () => {
    if (!prev) return;
    try { await api.setRate(prev.rate, prev.date); setMsg(`Restored ${prev.rate} JPY per 1 USD as of ${prev.date}.`); setPrev(null); onChanged(); } catch (e) { setMsg(errorText(e)); }
  };
  return (
    <form className="ma-calc" aria-label="Exchange rate" onSubmit={(e) => { e.preventDefault(); void save(); }}>
      <p className="pk-note" style={{ flexBasis: "100%" }}>{fx ? <>Stored rate: {fx.rate} JPY per 1 USD, as of {fx.date} ({SOURCE[fx.source] ?? fx.source}). Money in JPY and USD is only combined with a stored rate and its date.</> : "No exchange rate stored, so JPY and USD are never combined."}</p>
      <TextField label="JPY per 1 USD" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} error={errs.rate} />
      <TextField label="Rate date" value={date} placeholder="YYYY-MM-DD" onChange={(e) => setDate(e.target.value)} error={errs.date} />
      <Button type="submit" size="small">Save rate</Button>
      {prev && <Button type="button" size="small" onClick={() => { void undo(); }}>Undo</Button>}
      {msg && <p className="pk-note" role="status" style={{ flexBasis: "100%" }}>{msg}</p>}
    </form>
  );
}
