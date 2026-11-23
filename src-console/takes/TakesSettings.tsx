import { useState } from "react";
import { Card, DataTag, Toggle } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { takesAdapter, type TakesAdapter } from "./adapters";
import "./takes.css";
import { useTakeLogVersion } from "./takeLog";

/** Settings › Memory › Takes: how long takes are kept, and whether Amara may mark a suggestion Done from data (Taisun: yes, with Undo). */
export function TakesSettingsCard({ adapter = takesAdapter() }: { adapter?: TakesAdapter }) {
  useTakeLogVersion(); // re-render when the suggestion layer loads
  const w = adapter.settings();
  const v = valueOf(w);
  const [auto, setAuto] = useState<boolean | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const on = auto ?? v?.autoDone ?? false;
  return (
    <Card title="Takes" accessory={<span className="pk-acc">{w.state === "sample" ? <DataTag kind="sample" /> : w.state === "unwired" ? <DataTag kind="wire" /> : null}</span>}>
      <div className="pk-row"><div className="pk-row__main"><span>Keep every take, suggestion and answer for 1 year</span><small>Then Amara summarizes them into Memory and removes the daily rows. Stored on this Mac only.</small></div></div>
      <div className="pk-row"><div className="pk-row__main"><span id="takes-auto">Mark Done automatically when the data shows it</span><small>e.g. a WHOOP walk or a Muse session. Every auto-Done shows what it saw and has Undo.</small></div>
        {v ? <Toggle aria-labelledby="takes-auto" checked={on} onCheckedChange={(x) => { setAuto(x); void adapter.setAutoDone(x).then((r) => setNote(r.state === "unwired" ? "Needs wiring, so nothing changed." : `${r.state === "sample" ? "Sample data: " : ""}${x ? "Amara marks Done from data." : "You mark Done yourself."}`)); }} /> : <DataTag kind="wire" />}</div>
      {note && <p className="pk-note" role="status">{note}</p>}
    </Card>
  );
}
