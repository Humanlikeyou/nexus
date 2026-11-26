import { useState } from "react";
import { Button, Card, PageHero, Select, Toggle } from "../ui";
import type { NfProtocol } from "../legacy-core/lib/types";
import { describeFeature, PROTOCOL_SET_VERSION, targetFeatures, type ProtocolSettings, type ProtocolSound } from "./protocolConfig";
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

export function TrainingView({ training, running = false, onDone }: { training: TrainingApi; running?: boolean; onDone?: () => void }) {
  return (
    <>
      <PageHero title="Training" statement="Protocols, settings and your session records" detail="Neurofeedback follows the EEG features listed for each protocol. Everything here is optional." />
      <ProtocolSettingsCard training={training} disabled={running} />
      {onDone && <div className="pk-actions"><Button size="small" variant="plain" onClick={onDone}>Back to Neuro</Button></div>}
    </>
  );
}
