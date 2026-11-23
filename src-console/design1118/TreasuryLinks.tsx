import type { Route } from "../app/routes";
import { useStoresTick } from "../lib/localStores";
import { minorText } from "./TreasuryView";
import { enterpriseAdapter, type EnterpriseLive } from "./enterpriseLive";
import "./design1118.css";

/**
 * Treasury figures for the rest of Enterprise (decision 0042, gap 13). Backend figures only, in the
 * currency the backend gives (never a yen sign); anything missing stays out, never 0.
 */
const STATE_TEXT: Record<string, string> = {
  stale: "Some data is stale", flags: "Something to look at", ok: "All good", no_data: "Nothing imported yet",
};

/** Finances: the Treasury monitor state with its "as of" date, and a way into Treasury. Hidden until the ledger loaded. */
export function TreasuryFinancesLink({ onOpen, adapter }: { onOpen?: (route: Route) => void; adapter?: EnterpriseLive }) {
  useStoresTick();
  const f = (adapter ?? enterpriseAdapter()).finances();
  if (f.state === "unwired") return null;
  const st = f.value.monitorState;
  const text = st ? STATE_TEXT[st] ?? st : "Treasury has no store yet";
  return (
    <section className="d18-card" aria-labelledby="d18-fin-tr" data-testid="finances-treasury">
      <div className="d18-map__head"><h2 id="d18-fin-tr" className="d18-h">Treasury</h2></div>
      <p className="d18-note" data-testid="finances-monitor">{text}{st === "ok" && f.value.monitorAsOf ? `, ok as of ${f.value.monitorAsOf}` : ""}.</p>
      {onOpen && <button type="button" className="v-link" onClick={() => onOpen({ id: "enterprise", view: "treasury" })}>Open Treasury ›</button>}
    </section>
  );
}

type Venture = NonNullable<ReturnType<EnterpriseLive["ventures"]> extends infer W ? (W extends { value: infer V } ? V : never) : never>[number];

/** Venture page: this month's income, logged time and per hour (30 days), only the figures the backend has. */
export function VentureTreasuryCard({ venture, onOpen }: { venture: Venture; onOpen?: (route: Route) => void }) {
  const income = venture.incomeMonth.filter((m) => m.totalMinor !== null && m.currency);
  const hours = venture.perHour30d.filter((p) => p.perHourMinor !== null && p.currency);
  const mins = venture.attentionMinutes30d;
  if (income.length === 0 && mins == null && hours.length === 0) return null;
  return (
    <section className="d18-card" aria-labelledby="d18-v-tr" data-testid="venture-treasury">
      <div className="d18-map__head"><h2 id="d18-v-tr" className="d18-h">Treasury</h2></div>
      <ul className="d18-clist">
        {income.map((m) => <li key={`${m.streamId}-${m.currency}`}><div><b>Income this month</b><small>{minorText(m.totalMinor as number, m.currency)}</small></div></li>)}
        {mins != null && <li><div><b>Time logged, 30 days</b><small>{Math.round(mins / 6) / 10} hours</small></div></li>}
        {hours.map((p) => <li key={p.streamId}><div><b>Per hour, 30 days</b><small>{minorText(p.perHourMinor as number, p.currency)} an hour{p.fxStale ? " · rate is out of date" : ""}</small></div></li>)}
      </ul>
      {onOpen && <button type="button" className="v-link" onClick={() => onOpen({ id: "enterprise", view: "treasury" })}>Open Treasury ›</button>}
    </section>
  );
}
