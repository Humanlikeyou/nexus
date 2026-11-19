import { useEffect, useState } from "react";
import {
  liveTreasury, liveTreasuryMonitor, loadTreasury, loadTreasuryMonitor,
  type TreasuryAdapter, type TreasuryMonitorAdapter,
} from "../business/treasury";
import { inTauri } from "../lib/localStores";
import { PageHero } from "../ui";
import "./design1118.css";

/**
 * Enterprise › Treasury (decision 0032 #4). Read-only: streams, this month's income, per hour, flags
 * (stale first, "ok as of"), and the monthly review (hidden unless it passed the number check).
 * No figure is invented: empty states until data.
 */
const KIND_LABEL: Record<string, string> = {
  stale_data: "Data is stale", failed_import: "An import failed", income_down: "Income is down", no_income: "No income",
  results_spike: "Results spiked", no_conversions: "No conversions",
};
const label = (k: string) => KIND_LABEL[k] ?? k.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase());
/** Money from minor units, with the currency code (never a yen sign; a rate is needed for dollars). */
export const minorText = (minor: number, currency: string | null) => {
  const c = currency ?? "";
  const v = c === "JPY" ? minor : minor / 100;
  return `${c ? `${c} ` : ""}${v.toLocaleString("en-US", { maximumFractionDigits: c === "JPY" ? 0 : 2 })}`;
};

const ym = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
const ymd = (d: Date) => `${ym(d)}-${String(d.getDate()).padStart(2, "0")}`;

export function TreasuryBody({ treasury, monitor }: { treasury: TreasuryAdapter; monitor: TreasuryMonitorAdapter }) {
  const streams = treasury.streams(), months = treasury.incomeMonths(), perHour = treasury.perHour();
  const flags = monitor.flags(), status = monitor.status(), review = monitor.review();
  const name = (id: string) => (streams.state === "unwired" ? id : streams.value.find((s) => s.id === id)?.name ?? id);
  const thisMonth = ym(new Date());
  const incomeNow = months.state === "unwired" ? [] : months.value.filter((m) => m.month === thisMonth && m.totalMinor !== null);
  return (
    <>
      <section className="d18-card" aria-labelledby="tr-status">
        <div className="d18-map__head"><h2 id="tr-status" className="d18-h">Monitor</h2></div>
        {status.state === "unwired" ? <p className="d18-note">Nothing has been imported yet, so there is nothing to check.</p>
          : status.value.state === "ok" ? <p className="d18-note" data-testid="tr-ok">All good, ok as of {status.value.asOf}.</p>
            : status.value.state === "stale" ? <p className="d18-note" data-testid="tr-stale">Some data is stale. Re-import to refresh it.</p>
              : <p className="d18-note">{flags.state === "unwired" ? 0 : flags.value.length} to look at.</p>}
        {flags.state !== "unwired" && flags.value.length > 0 && (
          <ul className="d18-clist" aria-label="Flags">
            {flags.value.map((f) => <li key={f.id}><div><b>{label(f.kind)}</b><small>{f.streamId ? `${name(f.streamId)} · ` : ""}since {f.since.slice(0, 10)}</small></div></li>)}
          </ul>
        )}
      </section>
      <section className="d18-card" aria-labelledby="tr-streams">
        <div className="d18-map__head"><h2 id="tr-streams" className="d18-h">Streams</h2></div>
        {streams.state === "unwired" ? <p className="d18-note">No income streams yet. They appear once one is set up.</p> : (
          <ul className="d18-clist">
            {streams.value.map((s) => <li key={s.id}><div><b>{s.name}</b><small>{s.streamType} · {s.status}</small></div></li>)}
          </ul>
        )}
      </section>
      <section className="d18-card" aria-labelledby="tr-income">
        <div className="d18-map__head"><h2 id="tr-income" className="d18-h">Income this month</h2></div>
        {incomeNow.length === 0 ? <p className="d18-note">No income recorded this month yet.</p> : (
          <ul className="d18-clist">{incomeNow.map((m) => <li key={`${m.streamId}-${m.currency}`}><div><b>{name(m.streamId)}</b><small>{minorText(m.totalMinor as number, m.currency)}</small></div></li>)}</ul>
        )}
      </section>
      <section className="d18-card" aria-labelledby="tr-hour">
        <div className="d18-map__head"><h2 id="tr-hour" className="d18-h">Per hour</h2></div>
        {perHour.state === "unwired" ? <p className="d18-note">Per-hour needs logged time and income for the same stream.</p> : (
          <ul className="d18-clist">
            {perHour.value.map((p) => <li key={p.streamId}><div><b>{name(p.streamId)}</b>
              <small>{p.perHourMinor == null ? (p.reason ?? "Not enough data yet") : `${minorText(p.perHourMinor, p.currency)} an hour`}{p.fxStale ? " · rate is out of date" : ""}</small></div></li>)}
          </ul>
        )}
      </section>
      {review.state !== "unwired" && (
        <section className="d18-card" aria-labelledby="tr-review" data-testid="tr-review">
          <div className="d18-map__head"><h2 id="tr-review" className="d18-h">Monthly review · {review.value.month}</h2></div>
          <p className="d18-note" style={{ whiteSpace: "pre-wrap" }}>{review.value.text}</p>
        </section>
      )}
    </>
  );
}

export function TreasuryView() {
  const [snap, setSnap] = useState<{ t: TreasuryAdapter; m: TreasuryMonitorAdapter } | null>(() => (inTauri() ? null : { t: liveTreasury(null), m: liveTreasuryMonitor(null) }));
  useEffect(() => {
    if (!inTauri()) return;
    let live = true;
    const now = new Date(), from = new Date(now.getFullYear(), now.getMonth() - 3, 1);
    const prev = new Date(now.getFullYear(), now.getMonth() - 1, 1);
    void Promise.all([
      loadTreasury({ fromMonth: ym(from), toMonth: ym(now), from: ymd(from), to: ymd(now) }),
      loadTreasuryMonitor(ym(prev)),
    ]).then(([t, m]) => { if (live) setSnap({ t: liveTreasury(t), m: liveTreasuryMonitor(m) }); }).catch(() => { if (live) setSnap({ t: liveTreasury(null), m: liveTreasuryMonitor(null) }); });
    return () => { live = false; };
  }, []);
  return (
    <div className="biz page-inset cc-page d18-page">
      <PageHero title="Treasury" statement="Income streams" detail="Read-only. Amounts come from imports you make; nothing is moved or sent." />
      {snap ? <TreasuryBody treasury={snap.t} monitor={snap.m} /> : <p className="d18-note" role="status">Loading…</p>}
    </div>
  );
}
