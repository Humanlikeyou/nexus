import { useMemo, useState } from "react";
import { Button, Card, TextField } from "../ui";
import { askMore } from "../amaraTake/takeView";
import { todayJst } from "../ui/charts/chartMath";
import { NFA_LINE } from "./adapters";
import { adviceContext, buildAdvice, canAfford, type AdviceItem } from "./advice";
import { parseAmount, type WireCurrencyView } from "./entries";
import "./moneyCharts.css";

const CONF: Record<AdviceItem["confidence"], string> = { low: "Low confidence", medium: "Medium confidence", high: "High confidence" };

function Item({ item }: { item: AdviceItem }) {
  return (
    <li className="ma-item">
      <p className="ma-claim">{item.claim}</p>
      <p className="ma-meta">{CONF[item.confidence]} · from {item.numbers.map((n) => n.source).filter((s, i, a) => a.indexOf(s) === i).slice(0, 2).join("; ")}</p>
    </li>
  );
}

/** "Amara's money read": deterministic, from recorded facts only. Nothing here moves money. */
export function MoneyAdvice({ cv, today = todayJst() }: { cv: WireCurrencyView; today?: string }) {
  const c = cv.currency;
  const r = useMemo(() => buildAdvice(cv, today), [cv, today]);
  const [amount, setAmount] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [calc, setCalc] = useState<ReturnType<typeof canAfford> | null>(null);
  const check = () => {
    const minor = parseAmount(amount, c);
    if (minor == null) { setCalc(null); setErr(c === "JPY" ? "Enter a whole number of yen." : "Enter an amount with up to two decimals."); return; }
    setErr(null); setCalc(canAfford(cv, today, minor));
  };
  const ask = () => {
    const items = [...r.items, ...(calc?.items ?? [])];
    askMore({ from: "Money", chips: items.flatMap((i) => i.numbers.slice(0, 1).map((n) => ({ label: n.label, value: n.shown }))).slice(0, 6), question: "What should I look at first in this money read? Quote only these numbers and say what is missing.", sample: false, text: `${adviceContext(r)}${calc?.items.length ? `\n${calc.items.map((i) => `- ${i.claim}`).join("\n")}` : ""}\n${NFA_LINE}` });
  };
  const assumptions = [...new Set([...r.items, ...(calc?.items ?? [])].flatMap((i) => i.assumptions))];
  return (
    <Card title="Amara's money read" accessory={<span className="pk-acc"><Button size="small" onClick={ask}>Ask Amara</Button></span>}>
      {r.status === "needs_data" ? (
        <div role="status" className="ma-needs">
          <p className="ma-claim">Not enough recorded yet to say anything useful in {c}.</p>
          <ul>{r.needs.map((n) => <li key={n}>{n}</li>)}</ul>
        </div>
      ) : (
        <>
          <ul className="ma-list" aria-label={`Money read in ${c}`}>{r.items.map((i) => <Item key={i.id} item={i} />)}</ul>
          {r.hints.length > 0 && <p className="ma-meta">To add more: {r.hints.join(" ")}</p>}
        </>
      )}
      <form className="ma-calc" onSubmit={(e) => { e.preventDefault(); check(); }} aria-label="Can I afford it?">
        <TextField label={`Can I afford it? Amount (${c})`} inputMode="decimal" value={amount} onChange={(e) => { setAmount(e.target.value); setCalc(null); }} error={err ?? undefined} />
        <Button type="submit" size="small">Show the effect</Button>
      </form>
      <div role="status" aria-live="polite">
        {calc?.status === "needs_data" && <ul className="ma-needs">{calc.needs.map((n) => <li key={n}>{n}</li>)}</ul>}
        {calc?.status === "ok" && <ul className="ma-list">{calc.items.map((i) => <Item key={i.id} item={i} />)}</ul>}
      </div>
      {assumptions.length > 0 && <details className="ma-assume"><summary>Assumptions</summary><ul>{assumptions.map((a) => <li key={a}>{a}</li>)}</ul>
        <ul>{[...r.items, ...(calc?.items ?? [])].map((i) => <li key={i.id}><b>{i.id}:</b> {i.numbers.map((n) => `${n.label} ${n.shown} (${n.source})`).join("; ")}</li>)}</ul></details>}
      <p className="m-nfa" role="note">{NFA_LINE}</p>
    </Card>
  );
}
