import { useState } from "react";
import { BrainCircuit, HeartPulse, RefreshCw } from "lucide-react";
import { deviceTitle } from "../lib/devices";
import { dateTime } from "../lib/format";
import type { MuseDevice, SystemSnapshot } from "../types";
import { Badge, Button, InsetGroup, ListRow, type Tone } from "../ui";
import type { MuseOperation } from "./muse";
import { rssiText } from "./NeuroScreen";

/** Settings › Devices (screen-specs §9). Muse + NOOP band rows; pairing for NOOP stays in NOOP. */
export type DevicesGroupProps = {
  data: SystemSnapshot;
  devices: MuseDevice[];
  operation: MuseOperation;
  error: string | null;
  onScan: () => void;
  onConnect: (device: MuseDevice) => void;
  onDisconnect: () => void;
  onRetry?: () => void;
};

function noopBadge(status: SystemSnapshot["noop"]["status"]): { tone: Tone; label: string } {
  if (status === "ready") return { tone: "success", label: "Current" };
  if (status === "waiting_for_nightly_data" || status === "waiting_for_data") return { tone: "info", label: "Syncing" };
  if (status === "stale") return { tone: "warning", label: "Stale" };
  return { tone: "neutral", label: "Unavailable" };
}

export function DevicesGroup({ data, devices, operation, error, onScan: scan, onConnect, onDisconnect, onRetry }: DevicesGroupProps) {
  const { muse, noop } = data;
  const [scanned, setScanned] = useState(false);
  const onScan = () => { setScanned(true); scan(); };
  const scannedEmpty = scanned && operation === null && devices.length === 0 && !error;
  const noopDown = data.unavailableSources.some((s) => s.command === "get_noop_health_snapshot");
  const nb = noopBadge(noop.status);
  const battery = noop.battery ? `${Math.round(noop.battery.percentage)}% · ${noop.battery.charging ? "Charging" : "On strap"}` : "Battery unavailable";
  const footer = scannedEmpty && !muse.connected ? "No headsets found. Put the headset in pairing mode and try again." : "Amara OS reads these sensors locally.";
  return (
    <div id="devices" className="settings__devices">
      <InsetGroup title="Devices" footer={footer}>
        <ListRow icon={BrainCircuit} title={muse.device_name || "Muse headset"}
          subtitle={muse.streaming ? "Streaming EEG" : muse.connected ? "Connected and ready" : "Not connected"}
          accessory={
            <span className="settings__row-actions">
              <Badge tone={muse.streaming || muse.connected ? "success" : "neutral"}>{muse.streaming ? "Streaming" : muse.connected ? "Connected" : "Disconnected"}</Badge>
              {muse.connected
                ? <Button variant="secondary" size="small" onClick={onDisconnect} disabled={operation !== null}>Disconnect</Button>
                : <Button variant="primary" size="small" onClick={onScan} disabled={operation !== null}>{operation === "scanning" ? "Searching…" : "Find Muse"}</Button>}
            </span>
          } />
        <ListRow icon={HeartPulse} title={deviceTitle(noop.deviceId)}
          subtitle={noopDown ? "NOOP health unavailable" : noop.generatedAt ? `Updated ${dateTime(noop.generatedAt)} · Recovery, HRV, sleep, Effort` : noop.message || "No local NOOP record"}
          value={battery}
          accessory={noopDown && onRetry
            ? <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>
            : <Badge tone={nb.tone}>{nb.label}</Badge>} />
      </InsetGroup>
      {error && (
        <div className="settings__alert" role="alert">
          <p>{error}</p>
          <Button variant="secondary" size="small" onClick={onScan} disabled={operation !== null}>Retry</Button>
        </div>
      )}
      {devices.length > 0 && !muse.connected && (
        <InsetGroup title="Nearby headsets">
          {devices.map((device) => (
            <ListRow key={device.address} icon={BrainCircuit} title={device.name} subtitle={rssiText(device.rssi)}
              accessory={<Button size="small" variant="secondary" onClick={() => onConnect(device)} disabled={operation !== null}>Connect</Button>} />
          ))}
        </InsetGroup>
      )}
    </div>
  );
}

/** Legacy entry point (folded id "devices"); renders the same group. */
export function DevicesField(props: DevicesGroupProps) {
  return <div className="settings"><h1 className="settings__title">Devices</h1><DevicesGroup {...props} /></div>;
}
