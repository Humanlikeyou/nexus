import { useState } from "react";
import { Button, DataTag, Dialog, PageHero, StatusText, Toggle } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { dateTime } from "../lib/format";
import type { SystemSnapshot } from "../types";
import type { StandaloneAdapter } from "./adapters";
import { Row, SaCard, Spin, WireNote } from "./parts";

/** Wording for the transition lock (Taisun, 14:59 JST): health sync is part of Amara; only a legacy helper can still hold the strap. */
export const TAKEOVER_TITLE = "Another app is using your WHOOP; Amara will take over";

/**
 * Settings › Devices › WHOOP 4 (design-1111s s6a–s6c). Health sync is built into Amara.
 * The single-owner notice shows only during the transition, while a legacy helper still holds the strap.
 */
export function WhoopPanel({ adapter, data }: { adapter: StandaloneAdapter; data: SystemSnapshot }) {
  const [, bump] = useState(0);
  const [pairing, setPairing] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const ww = adapter.whoop();
  const w = valueOf(ww);
  const steps = valueOf(adapter.pairSteps());
  const locked = w?.owner === "legacy";
  const act = async (a: Parameters<StandaloneAdapter["whoopAction"]>[0], ok: string) => { const r = await adapter.whoopAction(a); setNote(r.state === "unwired" ? "That needs wiring. Nothing changed." : ok); bump((n) => n + 1); };
  // Live fallback: what the health snapshot already knows (battery, last update), until whoop_status exists.
  const snapBattery = data.noop.battery ? Math.round(data.noop.battery.percentage) : null;
  const battery = w?.batteryPct ?? snapBattery;
  const statement = !w ? "WHOOP 4" : locked ? "WHOOP 4 · Amara will take over" : w.connected ? "WHOOP 4 · Connected" : "WHOOP 4 · Not connected";
  const detail = !w ? <>{data.noop.generatedAt ? `Health data updated ${dateTime(data.noop.generatedAt)}` : "Pair, sync and battery for your strap"} <DataTag kind="wire" /></>
    : locked ? `Amara reads your health data for now · ${w.lastSync.toLowerCase()}` : `Synced ${w.lastSync.replace(/ ·.*/, "")} · battery ${battery ?? "—"}% · live heart rate ${w.liveHr === "live" ? "on" : "off"}`;
  return (
    <>
      <PageHero title="Devices" statement={statement} detail={detail} />
      {locked && (
        <div className="pk-lock" role="region" aria-label="WHOOP handover">
          <span className="pk-lock__ic" aria-hidden="true">🔒</span>
          <div>
            <h3>{TAKEOVER_TITLE}</h3>
            <p>Only one app can talk to the strap at a time, or data can be lost. Until Amara takes over, it reads your health data read-only, and nothing is missing. This notice goes away once the move is done.</p>
            <div className="pk-actions"><Button size="small" variant="primary" onClick={() => setConfirm(true)}>Let Amara Take Over…</Button><Button size="small" variant="secondary" onClick={() => setNote("OK. Amara keeps reading your health data and asks again later.")}>Not Now</Button>
              <span className="pk-note">Asks first. Your health history is copied and checked; the original stays 30 days.</span></div>
          </div>
        </div>
      )}
      <div className="pk-g2">
        <div className="pk-stack">
          <SaCard title="WHOOP 4" w={ww}>
            <div className="pk-device">
              <div className="pk-strap" data-off={!w?.connected || undefined}><span /></div>
              <div className="pk-stack sa-gap8">
                <div className="pk-row sa-row-flat"><div className="pk-row__main"><span className="sa-device-name">Your WHOOP 4.0</span><small>{!w ? "Health sync is built into Amara" : locked ? "Amara takes over from another app" : "Owned by Amara on this Mac · Bluetooth"}</small></div>
                  {!w ? <DataTag kind="wire" /> : locked ? <StatusText kind="wait">Handing over</StatusText> : w.connected ? <StatusText kind="ok">Connected</StatusText> : <StatusText kind="off">Not connected</StatusText>}</div>
                <div className="pk-battery"><span className="pk-battery__body" role="img" aria-label={battery != null ? `Battery ${battery}%` : "Battery unknown"}><i style={{ width: `${battery ?? 0}%` }} /></span><span className="pk-big sa-big17">{battery != null ? `${battery}%` : "—"}</span><span className="pk-note">{w ? w.batteryNote : snapBattery != null ? "from the health store" : "no reading yet"}</span></div>
                <div className="pk-actions">
                  <Button size="small" variant="secondary" disabled={!w || locked} onClick={() => { void act("sync", "Syncing."); }}>Sync Now</Button>
                  <Button size="small" variant="secondary" disabled={!w || locked} onClick={() => { void act("buzz", "Buzzed the strap."); }}>Buzz Strap</Button>
                  <Button size="small" variant="secondary" disabled={!w || locked} onClick={() => setPairing(true)}>Re-pair…</Button>
                  <Button size="small" variant="secondary" disabled={!w || locked} onClick={() => setNote("Forget asks first, and needs wiring.")}>Forget</Button>
                </div>
              </div>
            </div>
          </SaCard>
          <SaCard title="Sync" w={ww}>
            {w ? <>
              <Row title="Last sync" sub={w.lastSync}><StatusText kind="ok">{locked ? "Fresh" : "Up to date"}</StatusText></Row>
              <Row title="Live heart rate" sub={w.liveNote}>{w.liveHr === "live" ? <StatusText kind="run">Live</StatusText> : <StatusText kind="off">{w.liveHr === "delayed" ? "Delayed" : "Off"}</StatusText>}</Row>
              <Row title="History gaps" sub={w.gaps14d === 0 ? "None in the last 14 days" : `${w.gaps14d} in the last 14 days`}><StatusText kind={w.gaps14d === 0 ? "ok" : "wait"}>{w.gaps14d === 0 ? "None" : String(w.gaps14d)}</StatusText></Row>
              <Row title="Recovery, sleep, effort" sub={w.scores}><StatusText kind="ok">Current</StatusText></Row>
            </> : <WireNote>Needs wiring: last sync, live heart rate and history gaps come from Amara&apos;s health sync (HANDOFF-1111s §6).</WireNote>}
          </SaCard>
        </div>
        <div className="pk-stack">
          <SaCard title="Who has the strap">
            {w ? (locked ? <>
              <Row title="Amara (Health sync)" sub="Takes over when you say"><StatusText kind="off">Waiting</StatusText></Row>
              <Row title="Another app" sub="Still connected from before the move"><StatusText kind="wait">Using it now</StatusText></Row>
            </> : <Row title="Amara (Health sync)" sub="Pairs, syncs and buzzes the strap"><StatusText kind="ok">Owner</StatusText></Row>) : <Row title="Amara (Health sync)" sub="Built into Amara"><DataTag kind="wire" /></Row>}
            <p className="pk-note">Only one at a time, enforced with a lock file. <DataTag kind="wire" /></p>
          </SaCard>
          <SaCard title="Health store" w={ww}>
            {w ? (locked ? <>
              <Row title="Where" sub={w.store.where}><StatusText kind="ok">Read-only</StatusText></Row>
              <Row title="Move into Amara" sub="Happens when Amara takes over · copied and checked first"><StatusText kind="off">Not yet</StatusText></Row>
              <Row title="Lucid Dream cues" sub="Start once Amara has the strap"><StatusText kind="off">Paused</StatusText></Row>
            </> : <>
              <Row title="Where" sub={w.store.where}><StatusText kind={w.store.verified ? "ok" : "wait"}>{w.store.verified ? "Checked" : "Checking"}</StatusText></Row>
              {w.store.oldCopyUntil && <Row title="Previous copy" sub={`Kept until ${w.store.oldCopyUntil} · deleted only when you say`}><StatusText kind="wait">Kept</StatusText></Row>}
              <Row title="Lucid Dream cues" sub="From this Mac through the strap"><Toggle aria-label="Lucid cues" checked={w.lucid} onCheckedChange={(v) => { void act(v ? "lucid_on" : "lucid_off", v ? "Lucid cues on." : "Lucid cues off."); }} /></Row>
            </>) : <WireNote>Needs wiring: where your health history lives and when it was last checked.</WireNote>}
          </SaCard>
          <SaCard title="Other devices">
            <Row title="Muse headband" sub={w?.muse ?? "In-app Bluetooth"}>{data.muse.connected ? <StatusText kind="ok">Connected</StatusText> : <StatusText kind="ok">Ready</StatusText>}</Row>
          </SaCard>
        </div>
      </div>
      {note && <p className="pk-note" role="status">{note}</p>}
      <Dialog open={confirm} title="Let Amara take over your WHOOP?" destructive confirmLabel="Take Over" onCancel={() => setConfirm(false)} onConfirm={() => { setConfirm(false); void act("take_over", "Amara is taking over. Your health history is being copied and checked."); }}>
        <p>Amara asks the other app to let go of the strap (it never forces it closed), copies and checks your health history, then pairs. The original copy stays 30 days and is deleted only when you say.</p>
      </Dialog>
      <Dialog open={pairing} title="Pair your WHOOP 4" onCancel={() => setPairing(false)} className="sa-sheet">
        {steps ? <ol className="pk-steps">{steps.map((s) => <li key={s.id}><span>{s.title}<small>{s.sub}</small></span>{s.state === "done" ? <StatusText kind="ok">Done</StatusText> : s.state === "active" ? <Spin>Connecting</Spin> : <StatusText kind="wait">Next</StatusText>}</li>)}</ol> : <WireNote>Needs wiring: pairing steps come from whoop_pair_events.</WireNote>}
        <p>WHOOP 4 needs no account and no password. Re-pairing never deletes health history.</p>
        <div className="pk-sheet__actions"><Button size="small" variant="secondary" onClick={() => setPairing(false)}>Cancel</Button><Button size="small" variant="primary" disabled>Pair</Button></div>
      </Dialog>
    </>
  );
}
