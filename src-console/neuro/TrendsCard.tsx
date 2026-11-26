import { useEffect, useState } from "react";
import { invokeBound } from "../bridge";
import { Card } from "../ui";
import type { NfProtocol } from "../legacy-core/lib/types";
import "../ui/PanelKit.css";

/** Decision 0059 (step 8f): long-term trends, as `neuro_training_trends` returns them. Validated sessions only. */
export type FeatureTrend = {
  feature: string; unit: string; calculation: string; n: number; median: number; robustSd: number;
  earlierMedian: number; laterMedian: number; change: number; changeInSpreads: number | null; slopePerSession: number;
  direction: "higher" | "lower" | "no_clear_change"; confidence: string; uncertainty: string;
};
export type ProtocolTrends = { protocolId: string; method: string; sessions: number; first: string; last: string; features: FeatureTrend[]; sessionIds: string[] };
export type TrainingTrends = { version: string; records: number; eligible: number; protocols: ProtocolTrends[]; note: string };

const FEATURE: Record<string, string> = { final_threshold_scale: "Reward threshold at the end", valid_pct: "Usable signal" };
const featureName = (f: string) => FEATURE[f] ?? (f.startsWith("resting_level:") ? `${f.slice("resting_level:".length)} resting level` : f);
const value = (v: number, unit: string) => (unit === "x" ? `x${v.toFixed(2)}` : `${v.toFixed(1)}${unit}`);
const DIRECTION = { higher: "higher in later sessions", lower: "lower in later sessions", no_clear_change: "no clear change" } as const;
const day = (iso: string) => iso.slice(0, 10);

const loadTrends = () => invokeBound<TrainingTrends>("neuro_training_trends");

export function TrendsCard({ protocols, load = loadTrends }: { protocols: NfProtocol[]; load?: () => Promise<TrainingTrends> }) {
  const [t, setT] = useState<TrainingTrends | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { let alive = true; load().then((r) => { if (alive) setT(r); }, (e: unknown) => { console.error("neuro_training_trends", e); if (alive) setError("unreadable"); }); return () => { alive = false; }; }, [load]);
  const name = (id: string) => protocols.find((p) => p.id === id)?.name ?? id;
  return (
    <Card title="Trends over time" accessory={t ? <span className="pk-acc"><span className="v-note">{t.version}</span></span> : undefined}>
      {error ? <p className="pk-note" role="alert">The trends couldn&apos;t be read.</p>
        : !t ? <p className="pk-note" role="status">Reading the trends…</p>
        : <>
          <p className="pk-note">{t.eligible} of {t.records} recorded sessions are usable for trends.</p>
          {t.protocols.length === 0 && <p className="pk-note">No usable sessions yet.</p>}
          {t.protocols.map((p) => (
            <section key={`${p.protocolId}:${p.method}`} aria-label={`${name(p.protocolId)} trends`}>
              <p className="pk-note"><b>{name(p.protocolId)}</b> · {p.sessions} usable session{p.sessions === 1 ? "" : "s"}{p.sessions ? `, ${day(p.first)} to ${day(p.last)}` : ""} · <span className="v-note">{p.method}</span></p>
              {p.features.length === 0 ? <p className="pk-note">Not enough sessions for a trend yet (it needs five).</p> : (
                <table className="pk-table" aria-label={`${name(p.protocolId)} trend measures`}>
                  <thead><tr><th scope="col">Measure</th><th scope="col">Earlier</th><th scope="col">Later</th><th scope="col">Reading</th></tr></thead>
                  <tbody>{p.features.map((f) => (
                    <tr key={f.feature}><td>{featureName(f.feature)}</td><td>{value(f.earlierMedian, f.unit)}</td><td>{value(f.laterMedian, f.unit)}</td>
                      <td>{DIRECTION[f.direction]} · {f.n} sessions · confidence {f.confidence}</td></tr>
                  ))}</tbody>
                </table>
              )}
              {p.features.map((f) => <p key={f.feature} className="pk-note">{featureName(f.feature)}: {f.calculation}.</p>)}
              {p.features[0] && <p className="pk-note">{p.features[0].uncertainty}</p>}
            </section>
          ))}
          <p className="pk-note">{t.note}</p>
        </>}
    </Card>
  );
}
