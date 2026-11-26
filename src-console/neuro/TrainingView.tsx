import { useEffect, useState } from "react";
import { invokeBound } from "../bridge";
import { Button, Card, PageHero, Select, Toggle } from "../ui";
import type { NfProtocol } from "../legacy-core/lib/types";
import { describeFeature, PROTOCOL_SET_VERSION, targetFeatures, type ProtocolSettings, type ProtocolSound } from "./protocolConfig";
import { biofeedback, type Biofeedback } from "./biofeedback";
import { TrendsCard } from "./TrendsCard";
import { useMuseTelemetry } from "./useMuseTelemetry";
import "../ui/PanelKit.css";

/**
 * Decision 0059: Neuro › Training (design boundary lifted for this view only, as 0058 did for Calibration).
 * Protocol settings and their exact target features; later steps add the session records and trends.
 */
export const NOT_A_STATE = "These are measurements of the EEG signal. They are not measures of emotion, attention, stress or relaxation, and a change during one session does not show a lasting change.";

const DURATIONS = [5, 10, 15, 20, 25, 30, 45, 60].map((m) => ({ value: String(m), label: `${m} minutes` }));
const RATES = [40, 45, 50, 55, 60, 65, 70, 75, 80].map((p) => ({ value: String(p / 100), label: `${p}% of clean time` }));
const SOUNDS: Array<{ value: ProtocolSound; label: string }> = [{ value: "soundscape", label: "Soundscape" }, { value: "tone", label: "Tone" }, { value: "silent", label: "No sound" }];
const VOLUMES = Array.from({ length: 11 }, (_, i) => ({ value: String(i / 10), label: `${i * 10}%` }));

export type TrainingApi = {
  protocols: NfProtocol[];
  protocolId: string;
  settingsFor: (id: string) => ProtocolSettings | null;
  saveSettings: (id: string, settings: ProtocolSettings | null) => Promise<void>;
};

export function ProtocolSettingsCard({ training, disabled = false }: { training: TrainingApi; disabled?: boolean }) {
  const initial = training.protocols.some((p) => p.id === training.protocolId) ? training.protocolId : training.protocols[0]?.id ?? "";
  const [id, setId] = useState(initial);
  const protocol = training.protocols.find((p) => p.id === id);
  const current = training.settingsFor(id);
  const [draft, setDraft] = useState<ProtocolSettings | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  if (!protocol || !current) return null;
  const s = draft ?? current;
  const set = (patch: Partial<ProtocolSettings>) => { setDraft({ ...s, ...patch }); setMsg(null); };
  const save = (value: ProtocolSettings | null) => training.saveSettings(id, value).then(() => { setDraft(null); setMsg(value ? "Saved. The next session uses these settings." : "Back to this protocol's defaults."); }, (e: unknown) => setMsg(e instanceof Error ? e.message : String(e)));
  return (
    <Card title="Protocol settings" accessory={<span className="pk-acc"><span className="v-note">{PROTOCOL_SET_VERSION}</span></span>}>
      <Select label="Protocol" options={training.protocols.map((p) => ({ value: p.id, label: p.name }))} value={id} onChange={(e) => { setId(e.target.value); setDraft(null); setMsg(null); }} />
      <p className="pk-note">{protocol.description}</p>
      <ul className="pk-list" aria-label="Target features">
        {targetFeatures(protocol).map((f) => <li key={f.id}>{describeFeature(f)}{f.declaredHz ? ` (the protocol names ${f.declaredHz[0]}–${f.declaredHz[1]} Hz; the training computes the range shown)` : ""}</li>)}
      </ul>
      <p className="pk-note">Measured as relative band power on the forehead electrodes, against this session&apos;s own baseline. {NOT_A_STATE}</p>
      <Select label="Planned length" options={DURATIONS} value={String(s.durationMin)} disabled={disabled} onChange={(e) => set({ durationMin: Number(e.target.value) })} />
      <Select label="Target reward rate" options={RATES} value={String(s.targetRewardRate)} disabled={disabled} onChange={(e) => set({ targetRewardRate: Number(e.target.value) })} />
      <Select label="Sound" options={SOUNDS} value={s.sound} disabled={disabled} onChange={(e) => set({ sound: e.target.value as ProtocolSound })} />
      <Select label="Volume" options={VOLUMES} value={String(Math.round(s.volume * 10) / 10)} disabled={disabled} onChange={(e) => set({ volume: Number(e.target.value) })} />
      <Toggle checked={s.visualFeedback} disabled={disabled} onCheckedChange={(v) => set({ visualFeedback: v })}>Visual feedback (optional)</Toggle>
      <div className="pk-actions">
        <Button size="small" variant="primary" disabled={disabled || !draft} onClick={() => { void save(s); }}>Save settings</Button>
        <Button size="small" variant="plain" disabled={disabled} onClick={() => { void save(null); }}>Use defaults</Button>
      </div>
      {disabled && <p className="pk-note">Settings can be changed when no session is running.</p>}
      {msg && <p className="pk-note" role="status">{msg}</p>}
    </Card>
  );
}

/** One saved session's progress record, as `neuro_training_records` returns it (no raw EEG). */
export type TrainingRecord = {
  sessionId: string; start: string; durationSecs: number; protocolId: string; protocolSet: string; recordVersion: string; method: string;
  source: string | null; validPct: number | null; timeInTargetPct: number | null; trainedSeconds: number; targetRewardRate: number | null;
  thresholdChecks: number; finalThresholdScale: number | null; baselineLevels: Record<string, number>; heldSeconds: Record<string, number>;
  eligible: boolean; notEligibleReason: string | null;
};

const HELD_LABEL: Record<string, string> = { contact: "contact", dropout: "signal gaps", blink: "blinks", jaw: "jaw", movement: "movement", noise: "noise", frozen: "frozen signal", not_live: "not live" };
const when = (iso: string) => { const d = new Date(iso); return Number.isNaN(d.getTime()) ? iso : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }); };
const num1 = (v: number | null | undefined, unit = "") => (v == null ? "Not measured" : `${Math.round(v * 10) / 10}${unit}`);

const loadRecords = () => invokeBound<TrainingRecord[]>("neuro_training_records", { limit: 20 });

export function TrainingRecordsCard({ protocols, load = loadRecords }: { protocols: NfProtocol[]; load?: () => Promise<TrainingRecord[]> }) {
  const [records, setRecords] = useState<TrainingRecord[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { let alive = true; load().then((r) => { if (alive) setRecords(r ?? []); }, (e: unknown) => { console.error("neuro_training_records", e); if (alive) setError("unreadable"); }); return () => { alive = false; }; }, [load]);
  const name = (id: string) => protocols.find((p) => p.id === id)?.name ?? id;
  return (
    <Card title="Session records">
      <p className="pk-note">How each guided session&apos;s feedback was produced. Time in target is kept near your target reward rate by design, because the threshold adapts, so it is a record of the session, not a score.</p>
      {error ? <p className="pk-note" role="alert">The records couldn&apos;t be read.</p>
        : records == null ? <p className="pk-note" role="status">Reading the records…</p>
        : records.length === 0 ? <p className="pk-note">No records yet. Records start with guided sessions from this version on.</p>
        : <ul className="pk-list" aria-label="Session records">{records.map((r) => {
          const held = Object.entries(r.heldSeconds).filter(([, s]) => s > 0).map(([k, s]) => `${HELD_LABEL[k] ?? k} ${Math.round(s)} s`);
          const levels = Object.entries(r.baselineLevels).map(([k, v]) => `${k} ${v.toFixed(1)}%`);
          return (
            <li key={r.sessionId}>
              <b>{when(r.start)} · {name(r.protocolId)}</b> ({r.source ?? "source not recorded"})
              <br />Usable {num1(r.validPct, "%")} of training · {Math.round(r.trainedSeconds / 60)} min trained · in target {num1(r.timeInTargetPct, "%")} (aim {r.targetRewardRate == null ? "not recorded" : `${Math.round(r.targetRewardRate * 100)}%`})
              <br />Resting level before training: {levels.length ? levels.join(", ") : "not recorded"} · {r.thresholdChecks} threshold checks, ending at {r.finalThresholdScale == null ? "not recorded" : `x${r.finalThresholdScale.toFixed(2)}`} of the baseline threshold
              {held.length > 0 && <><br />Held: {held.join(", ")}</>}
              <br /><span className="v-note">{r.eligible ? "Counts toward your trends." : `Not used for trends: ${r.notEligibleReason}.`} {r.method} · {r.protocolSet}</span>
            </li>
          );
        })}</ul>}
    </Card>
  );
}

const SENSOR_ROWS: Array<[keyof Omit<Biofeedback, "heartRate">, string]> = [["eeg", "EEG (Muse)"], ["imu", "Motion (Muse gyroscope)"], ["ppg", "PPG (Muse forehead sensor)"], ["rrIntervals", "Beat-to-beat (RR) intervals"], ["hrv", "Heart rate variability"]];

/** Decision 0059: biofeedback, apart from neurofeedback. Only the sensors that are really arriving now. */
export function BiofeedbackCard({ read }: { read: Biofeedback }) {
  return (
    <Card title="Biofeedback sensors">
      <p className="pk-note">Biofeedback follows body signals and is kept apart from neurofeedback, which follows the EEG features above. Nothing is assumed to be connected.</p>
      <ul className="pk-list" aria-label="Biofeedback sensors">
        {SENSOR_ROWS.map(([k, label]) => <li key={k}><b>{label}</b>: {read[k].state === "available" ? "available" : "not available"}. {read[k].detail}</li>)}
      </ul>
      <p className="pk-note" role="status">{read.heartRate ? `Heart rate ${read.heartRate.bpm} bpm (an estimate from the forehead PPG, not a medical reading).` : "No heart-rate reading right now."}</p>
    </Card>
  );
}

function LiveBiofeedback() {
  const telemetry = useMuseTelemetry();
  // A once-a-second clock, so a sensor that stops sending reads as not available without new telemetry.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => { const id = window.setInterval(() => setNow(Date.now()), 1000); return () => window.clearInterval(id); }, []);
  return <BiofeedbackCard read={biofeedback(telemetry, now)} />;
}

export function TrainingView({ training, running = false, onDone, onRecordAfter }: { training: TrainingApi; running?: boolean; onDone?: () => void; onRecordAfter?: () => void }) {
  return (
    <>
      <PageHero title="Training" statement="Protocols, settings and your session records" detail="Neurofeedback follows the EEG features listed for each protocol. Everything here is optional." />
      <ProtocolSettingsCard training={training} disabled={running} />
      <TrainingRecordsCard protocols={training.protocols} />
      <TrendsCard protocols={training.protocols} />
      <LiveBiofeedback />
      {onRecordAfter && (
        <Card title="Before and after a session">
          <p className="pk-note">Optional. Make a resting recording in Calibration before a session and another here after it, and the two are compared feature by feature. It shows a measured change in the signal, not an improvement.</p>
          <div className="pk-actions"><Button size="small" variant="secondary" disabled={running} onClick={onRecordAfter}>Record an after-session rest</Button></div>
        </Card>
      )}
      {onDone && <div className="pk-actions"><Button size="small" variant="plain" onClick={onDone}>Back to Neuro</Button></div>}
    </>
  );
}
