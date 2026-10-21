import { ActionButton, PageHeading, SectionHeading, StatusIndicator } from "../design/ProductUI";
import { tone, number, date } from "../lib/format";
import type { MuseScreenProps } from "./muse";
import { BrainCircuit, Bluetooth, HeartPulse, RefreshCw, Unplug } from "lucide-react";

export function DevicesField({ data, devices, operation, error, onScan, onConnect, onDisconnect }: Omit<MuseScreenProps, "telemetry" | "sessionStartedAt" | "sessionEndedAt" | "onStart" | "onStop">) {
  const noopReady = data.noop.status === "ready" || data.noop.status === "waiting_for_nightly_data" || data.noop.status === "stale";
  return <div className="system-field devices-field">
    <PageHeading eyebrow="Connections" title="Devices" description="Manage the sensors Nexus reads from and understand their current state." actions={<ActionButton icon={RefreshCw} onClick={onScan} disabled={operation !== null}>{operation === "scanning" ? "Searching..." : "Scan for Muse"}</ActionButton>} />
    {error && <div className="inline-alert inline-alert--danger" role="alert">{error}</div>}
    <section className="integration-list">
      <article className="integration-row">
        <div className="integration-icon"><BrainCircuit size={22} /></div>
        <div className="integration-main"><div><span>Muse</span><h2>{data.muse.device_name || "Muse headset"}</h2></div><p>{data.muse.connected ? data.muse.streaming ? "EEG data is flowing into an active session." : "Connected and ready for a live session." : "Pair a Muse headset for live EEG and session recording."}</p><div className="integration-capabilities">EEG · Frequency bands · Session recording · Neurofeedback</div></div>
        <div className="integration-state"><StatusIndicator tone={data.muse.streaming || data.muse.connected ? "success" : "neutral"}>{data.muse.streaming ? "Streaming" : data.muse.connected ? "Connected" : "Disconnected"}</StatusIndicator><span>{data.muse.connected ? "Live connection" : "No headset selected"}</span></div>
        <div className="integration-actions">{data.muse.connected ? <ActionButton icon={Unplug} onClick={onDisconnect} disabled={operation !== null}>Disconnect</ActionButton> : <ActionButton icon={Bluetooth} variant="primary" onClick={onScan} disabled={operation !== null}>Find Muse</ActionButton>}</div>
      </article>
      <article className="integration-row">
        <div className="integration-icon"><HeartPulse size={22} /></div>
        <div className="integration-main"><div><span>WHOOP via NOOP</span><h2>{data.noop.deviceId || "WHOOP"}</h2></div><p>{noopReady ? "Nexus is reading your local NOOP health record. Pairing remains managed by NOOP." : data.noop.message || "No current NOOP record is available."}</p><div className="integration-capabilities">Recovery · HRV · Sleep · Strain · Battery</div></div>
        <div className="integration-state"><StatusIndicator tone={data.noop.status === "ready" ? "success" : data.noop.status === "stale" || data.noop.status === "waiting_for_nightly_data" ? "warning" : "neutral"}>{data.noop.status === "ready" ? "Current" : data.noop.status === "waiting_for_nightly_data" ? "Syncing" : data.noop.status === "stale" ? "Data stale" : "Unavailable"}</StatusIndicator><span>{data.noop.generatedAt ? `Updated ${date(data.noop.generatedAt)}` : "No local update"}</span></div>
        <div className="integration-actions"><span className="battery-readout">{data.noop.battery ? `${number(data.noop.battery.percentage)}% battery` : "Battery unavailable"}</span></div>
      </article>
    </section>
    {devices.length > 0 && !data.muse.connected && <section className="available-devices"><SectionHeading title="Available Muse headsets" detail={`${devices.length} found nearby`} /><div>{devices.map((device) => <article key={device.address}><div><strong>{device.name}</strong><span>{device.rssi == null ? "Signal unknown" : `${device.rssi} dBm`}</span></div><ActionButton onClick={() => onConnect(device)} disabled={operation !== null}>Connect</ActionButton></article>)}</div></section>}
    <details className="diagnostics-disclosure device-diagnostics"><summary>Advanced diagnostics</summary><dl><div><dt>Muse transport</dt><dd>{data.muse.connected ? "Bluetooth Low Energy connected" : "Bluetooth Low Energy idle"}</dd></div><div><dt>NOOP source</dt><dd>Read-only local store</dd></div><div><dt>NOOP latest day</dt><dd>{data.noop.latestDay || "No record"}</dd></div><div><dt>Unavailable sources</dt><dd>{data.unavailableSources.length ? data.unavailableSources.map((source) => source.label).join(", ") : "None"}</dd></div></dl></details>
  </div>;
}
