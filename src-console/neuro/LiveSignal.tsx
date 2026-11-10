import { useState, type ReactNode } from "react";
import { assessChannel } from "../../src/hooks/useSignalQuality";
import type { Wired } from "../lib/sampleAdapters";
import { Card, DataTag, SegmentedControl, wiredTag } from "../ui";
import { EEG_CHANNELS, PLACEMENT, bucket, type AuxInfo, type Contact, type EegChannel } from "./recordings";
import { SignalLanes, centered, type LaneSpec } from "./SignalLanes";

/** design-1110e E9: the live incoming signal, every channel stacked, a calm fixed window (10 s default). */
export const LIVE_WINDOWS = [2, 5, 10] as const;
type LiveWindow = (typeof LIVE_WINDOWS)[number];
export const COLOR: Record<EegChannel, string> = { TP9: "var(--eeg-tp9)", AF7: "var(--eeg-af7)", AF8: "var(--eeg-af8)", TP10: "var(--eeg-tp10)", AUX: "var(--eeg-aux)" };
const contactOf = (values: number[]): Contact => (values.length < 64 ? "none" : assessChannel(values.slice(-256)) as Contact);

export function laneFor(channel: EegChannel, values: number[], contact: Contact | undefined): LaneSpec {
  return { key: channel, name: channel === "AUX" ? <>AUX <small>occipital</small></> : channel, label: channel === "AUX" ? "AUX occipital raw EEG" : `${channel} raw EEG`, where: PLACEMENT[channel], color: COLOR[channel], values, contact };
}

export function LiveSignal({ trace, aux, streaming, children }: { trace: Partial<Record<string, number[]>>; aux: Wired<AuxInfo>; streaming: boolean; children?: ReactNode }) {
  const [win, setWin] = useState<LiveWindow>(10);
  const auxOn = aux.state !== "unwired" && aux.value.detected && (trace.AUX?.length ?? 0) > 1;
  const channels: EegChannel[] = auxOn ? [...EEG_CHANNELS, "AUX"] : [...EEG_CHANNELS];
  const n = win * 256;
  const lanes = channels.map((c) => { const all = trace[c] ?? []; const recent = all.slice(-n); return laneFor(c, bucket(centered(recent), 900), contactOf(all)); });
  const any = lanes.some((l) => l.values.length > 1);
  const ticks = Array.from({ length: 6 }, (_, i) => (i === 5 ? "now" : `−${((5 - i) * win) / 5} s`));
  return (
    <Card title="Live signal · µV" className="neuro__signal rec-live" headingLevel={2}
      accessory={<span className="rec-acc"><span className="rec-badge" {...(auxOn ? { "data-aux": "" } : {})}><b>{channels.length}</b> channels{auxOn ? " · AUX detected" : ""}</span>
        <span className="rec-pills"><SegmentedControl label="Live window" size="small" value={String(win)} options={LIVE_WINDOWS.map((w) => ({ value: String(w), label: `${w} s` }))} onChange={(v) => setWin(Number(v) as LiveWindow)} /></span>{aux.state === "sample" && wiredTag(aux)}</span>}>
      {any
        ? <SignalLanes lanes={lanes} ticks={ticks} height={channels.length === 5 ? 52 : 56} fade caption={`last ${win} seconds`} />
        : <p className="neuro__pending">{streaming ? "Acquiring…" : "The live signal appears when a session starts."}</p>}
      <p className="rec-note">Fixed {win}-second window, newest on the right, updated 4 times a second. The same ±100 µV scale on every lane, so a loud sensor stands out instead of being rescaled; each lane has its offset removed.{" "}
        {auxOn ? null : aux.state === "unwired" ? <>AUX (back of head): automatic detection <DataTag kind="wire" /></> : "AUX (back of head): not detected. It appears on its own once the electrode makes good contact."}</p>
      {children}
    </Card>
  );
}
