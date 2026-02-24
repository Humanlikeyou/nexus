import { useState, useEffect, useRef, useCallback, useMemo } from "react";
import type {
  ElectrodeState,
  ElectrodeDeviceInfo,
  DeviceTelemetry,
  DataSource,
  ElectrodeMode,
} from "@/lib/types";
import { deriveElectrodeMode, isOutputAllowed } from "@/lib/electrode/electrodePolicy";
import {
  StubMyndliftAdapter,
  type ElectrodeAdapter,
} from "@/lib/electrode/myndliftAdapter";

const DEFAULT_STATE: ElectrodeState = {
  source: "muse",
  connected: false,
  streaming: false,
  mode: "off",
  status: "Idle",
  lastError: undefined,
  telemetry: null,
};

/**
 * Manages electrode device state, enforces safety policy, and provides
 * adapter lifecycle methods.
 *
 * This hook:
 * 1. Derives the safe electrode mode from system state (via electrodePolicy)
 * 2. Exposes scan/connect/disconnect/stream lifecycle
 * 3. Tracks device telemetry (battery, firmware, channels)
 * 4. Enforces "output locked off" invariant — isOutputAllowed() is always false
 *
 * The hook reads the current Muse connection state to determine if we're
 * in "read" mode. The Myndlift adapter is a stub for now.
 */
export function useElectrode(
  /** Whether the primary Muse is connected */
  museConnected: boolean,
  /** Whether Muse is streaming EEG */
  museStreaming: boolean,
  /** Current data source — live, replay, or demo */
  dataSource: DataSource,
  /** Score confidence from the DSP pipeline */
  confidence: number,
) {
  const [state, setState] = useState<ElectrodeState>(DEFAULT_STATE);
  const [discoveredDevices, setDiscoveredDevices] = useState<ElectrodeDeviceInfo[]>([]);
  const [scanning, setScanning] = useState(false);

  // Adapter ref — stub for now, will be swapped for real Myndlift SDK
  const adapterRef = useRef<ElectrodeAdapter>(new StubMyndliftAdapter());

  // Derive electrode mode from system state (safety policy)
  const derivedMode = useMemo<ElectrodeMode>(
    () => deriveElectrodeMode(museConnected, museStreaming, dataSource, confidence),
    [museConnected, museStreaming, dataSource, confidence],
  );

  // Update state when inputs change
  useEffect(() => {
    const source = dataSource === "demo" ? "demo" as const
      : dataSource === "replay" ? "replay" as const
      : "muse" as const;

    let status: string;
    if (!museConnected) {
      status = "Disconnected";
    } else if (!museStreaming) {
      status = "Connected — not streaming";
    } else if (derivedMode === "read") {
      status = "Reading EEG";
    } else {
      status = "Idle";
    }

    setState((prev) => ({
      ...prev,
      source,
      connected: museConnected,
      streaming: museStreaming,
      mode: derivedMode,
      status,
    }));
  }, [museConnected, museStreaming, dataSource, derivedMode]);

  // Myndlift device scanning (stub — returns empty for now)
  const scanForDevices = useCallback(async () => {
    setScanning(true);
    setState((prev) => ({ ...prev, lastError: undefined }));
    try {
      const devices = await adapterRef.current.scan();
      setDiscoveredDevices(devices);
    } catch (e) {
      setState((prev) => ({ ...prev, lastError: String(e) }));
    } finally {
      setScanning(false);
    }
  }, []);

  // Request telemetry from connected device
  const refreshTelemetry = useCallback(async () => {
    try {
      const telemetry = await adapterRef.current.getTelemetry();
      setState((prev) => ({ ...prev, telemetry }));
    } catch {
      // Telemetry is best-effort
    }
  }, []);

  // When Muse connects and starts streaming, build telemetry from known constants
  useEffect(() => {
    if (museConnected && museStreaming && dataSource === "live") {
      setState((prev) => ({
        ...prev,
        telemetry: prev.telemetry ?? {
          batteryPct: null,
          firmwareVersion: null,
          serialNumber: null,
          hardwareVersion: null,
          channels: ["TP9", "AF7", "AF8", "TP10"],
          sampleRate: 256,
        },
      }));
    } else if (!museConnected) {
      setState((prev) => ({ ...prev, telemetry: null }));
    }
  }, [museConnected, museStreaming, dataSource]);

  return {
    state,
    derivedMode,
    discoveredDevices,
    scanning,
    /** Output is always locked off at this stage */
    outputAllowed: isOutputAllowed(),
    scanForDevices,
    refreshTelemetry,
  };
}
