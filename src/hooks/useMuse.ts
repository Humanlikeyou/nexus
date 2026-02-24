import { useState, useEffect, useCallback, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  scanMuse,
  connectMuse,
  disconnectMuse,
  startEegStream,
  stopEegStream,
} from "@/lib/api";
import { EegRingBuffer } from "@/lib/eegRingBuffer";
import type { MuseDevice, EegSample, MuseConnectionState, BleFlowState, BleStatePayload, DataSource, EegChannel } from "@/lib/types";

const RING_BUFFER_SECS = 30; // store up to 30s for display window flexibility

export interface EegDebugInfo {
  packetsReceived: number;
  lastPacketAt: number | null;
  bufferLens: Record<string, number>;
  listenerActive: boolean;
}

export function useMuse() {
  const [devices, setDevices] = useState<MuseDevice[]>([]);
  const [connectionState, setConnectionState] = useState<MuseConnectionState>({
    connected: false,
    device_name: null,
    streaming: false,
  });
  const [scanning, setScanning] = useState(false);
  // Legacy eegData kept for FFT hook compatibility (throttled updates)
  const [eegData, setEegData] = useState<Record<string, number[]>>({
    TP9: [],
    AF7: [],
    AF8: [],
    TP10: [],
  });
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [bleState, setBleState] = useState<BleFlowState>("idle");
  const [dataSource, setDataSource] = useState<DataSource>("live");
  const [eegDebug, setEegDebug] = useState<EegDebugInfo>({
    packetsReceived: 0,
    lastPacketAt: null,
    bufferLens: { TP9: 0, AF7: 0, AF8: 0, TP10: 0 },
    listenerActive: false,
  });

  const unlistenRef = useRef<UnlistenFn | null>(null);
  const unlistenStateRef = useRef<UnlistenFn | null>(null);
  const packetCountRef = useRef(0);

  // Ring buffer: mutable ref, no re-renders on write
  const ringBufferRef = useRef<EegRingBuffer>(new EegRingBuffer(RING_BUFFER_SECS));

  // Throttled state sync for FFT (every 250ms, not every packet)
  const lastSyncRef = useRef(0);
  const FFT_SYNC_INTERVAL = 250; // ms

  // Listen to Rust-emitted BLE state (single source of truth)
  // Payload is now always { state: BleFlowState, data_source: DataSource }
  useEffect(() => {
    let cancelled = false;
    listen<BleStatePayload>("muse-ble-state", (event) => {
      if (!cancelled) {
        const payload = event.payload;
        setBleState(payload.state);
        setDataSource(payload.data_source);
        // Auto-update streaming state for replay/demo sources
        if (payload.state === "streaming" && payload.data_source !== "live") {
          setConnectionState((prev) => ({ ...prev, streaming: true }));
        } else if (payload.state === "idle" && payload.data_source !== "live") {
          setConnectionState((prev) => ({ ...prev, streaming: false }));
        }
      }
    }).then((unlisten) => {
      unlistenStateRef.current = unlisten;
    });
    return () => {
      cancelled = true;
      unlistenStateRef.current?.();
    };
  }, []);

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const found = await scanMuse();
      setDevices(found);
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }, []);

  const connect = useCallback(async (address: string) => {
    setConnecting(true);
    setError(null);
    try {
      const state = await connectMuse(address);
      setConnectionState(state);
    } catch (e) {
      setError(String(e));
    } finally {
      setConnecting(false);
    }
  }, []);

  const disconnect = useCallback(async () => {
    try {
      await stopEegStream().catch(() => {});
      await disconnectMuse();
      setConnectionState({ connected: false, device_name: null, streaming: false });
      setEegData({ TP9: [], AF7: [], AF8: [], TP10: [] });
      ringBufferRef.current.clear();
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const startStream = useCallback(async () => {
    try {
      packetCountRef.current = 0;
      ringBufferRef.current.clear();
      setEegDebug((prev) => ({ ...prev, packetsReceived: 0, lastPacketAt: null, listenerActive: true }));

      console.log("[useMuse] setting up muse-eeg listener");
      const unlisten = await listen<EegSample>("muse-eeg", (event) => {
        const { channel, samples } = event.payload;
        packetCountRef.current += 1;
        const now = Date.now();

        // Log first 5 packets and then every 100th
        if (packetCountRef.current <= 5 || packetCountRef.current % 100 === 0) {
          console.log(`[useMuse] muse-eeg #${packetCountRef.current}: ch=${channel} samples=${samples.length} first=${samples[0]?.toFixed(1)}`);
        }

        // Write to ring buffer (no React state update)
        ringBufferRef.current.push(channel as EegChannel, samples);

        // Throttled sync to eegData state for FFT hook
        if (now - lastSyncRef.current >= FFT_SYNC_INTERVAL) {
          lastSyncRef.current = now;
          const rb = ringBufferRef.current;
          setEegData({
            TP9: rb.read("TP9", 512),
            AF7: rb.read("AF7", 512),
            AF8: rb.read("AF8", 512),
            TP10: rb.read("TP10", 512),
          });
          setEegDebug({
            packetsReceived: packetCountRef.current,
            lastPacketAt: now,
            bufferLens: {
              TP9: rb.samplesAvailable("TP9"),
              AF7: rb.samplesAvailable("AF7"),
              AF8: rb.samplesAvailable("AF8"),
              TP10: rb.samplesAvailable("TP10"),
            },
            listenerActive: true,
          });
        }
      });
      unlistenRef.current = unlisten;
      console.log("[useMuse] listener active, calling startEegStream");

      await startEegStream();
      console.log("[useMuse] startEegStream returned OK");
      setConnectionState((prev) => ({ ...prev, streaming: true }));
    } catch (e) {
      console.error("[useMuse] startStream error:", e);
      setError(String(e));
    }
  }, []);

  const stopStream = useCallback(async () => {
    try {
      await stopEegStream();
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      setConnectionState((prev) => ({ ...prev, streaming: false }));
      setEegDebug((prev) => ({ ...prev, listenerActive: false }));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    return () => {
      if (unlistenRef.current) {
        unlistenRef.current();
      }
    };
  }, []);

  return {
    devices,
    connectionState,
    scanning,
    connecting,
    bleState,
    dataSource,
    eegData,
    eegDebug,
    error,
    ringBuffer: ringBufferRef.current,
    scan,
    connect,
    disconnect,
    startStream,
    stopStream,
  };
}
