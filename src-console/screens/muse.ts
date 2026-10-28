import type { MuseTelemetry } from "../neuro/telemetryStore";
import type { MuseDevice, SystemSnapshot } from "../types";

export type MuseOperation = "scanning" | "connecting" | "starting" | "stopping" | "disconnecting" | null;

export interface MuseScreenProps {
  data: SystemSnapshot;
  devices: MuseDevice[];
  operation: MuseOperation;
  error: string | null;
  sessionStartedAt: number | null;
  sessionEndedAt: number | null;
  onScan: () => void;
  onConnect: (device: MuseDevice) => void;
  onDisconnect: () => void;
  onStart: () => void;
  onStop: () => void;
}

export function museSignalState(data: SystemSnapshot, telemetry: MuseTelemetry) {
  if (!data.muse.streaming) return { tone: "neutral" as const, label: data.muse.connected ? "Ready" : "Disconnected" };
  if (!telemetry.lastPacketAt) return { tone: "warning" as const, label: "Acquiring signal" };
  if (Date.now() - telemetry.lastPacketAt > 2_500) return { tone: "danger" as const, label: "Signal interrupted" };
  return { tone: "success" as const, label: "Data flowing" };
}
