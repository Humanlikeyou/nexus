import { useState, useEffect, useCallback, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  scanWhoop,
  connectWhoop,
  disconnectWhoop,
  startHrStream,
  stopHrStream,
} from "@/lib/api";
import type { WhoopDevice, HeartRateData, WhoopConnectionState, BleFlowState } from "@/lib/types";

const MAX_HR_POINTS = 300; // 5 minutes of data at 1Hz

export function useWhoop() {
  const [devices, setDevices] = useState<WhoopDevice[]>([]);
  const [connectionState, setConnectionState] = useState<WhoopConnectionState>({
    connected: false,
    device_name: null,
    streaming: false,
  });
  const [scanning, setScanning] = useState(false);
  const [hrData, setHrData] = useState<HeartRateData[]>([]);
  const [currentHR, setCurrentHR] = useState<number | null>(null);
  const [rrIntervals, setRrIntervals] = useState<number[]>([]);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [bleState, setBleState] = useState<BleFlowState>("idle");
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const unlistenStateRef = useRef<UnlistenFn | null>(null);

  // Listen to Rust-emitted BLE state (single source of truth)
  useEffect(() => {
    let cancelled = false;
    listen<BleFlowState>("whoop-ble-state", (event) => {
      if (!cancelled) {
        setBleState(event.payload);
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
      const found = await scanWhoop();
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
      const state = await connectWhoop(address);
      setConnectionState(state);
    } catch (e) {
      setError(String(e));
    } finally {
      setConnecting(false);
    }
  }, []);

  const disconnect = useCallback(async () => {
    try {
      await stopHrStream().catch(() => {});
      await disconnectWhoop();
      setConnectionState({ connected: false, device_name: null, streaming: false });
      setHrData([]);
      setCurrentHR(null);
      setRrIntervals([]);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const startStream = useCallback(async () => {
    try {
      const unlisten = await listen<HeartRateData>("whoop-hr", (event) => {
        const data = event.payload;
        setCurrentHR(data.heart_rate);
        setHrData((prev) => [...prev.slice(-MAX_HR_POINTS), data]);
        if (data.rr_interval) {
          setRrIntervals((prev) => [...prev.slice(-60), data.rr_interval!]);
        }
      });
      unlistenRef.current = unlisten;

      await startHrStream();
      setConnectionState((prev) => ({ ...prev, streaming: true }));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const stopStream = useCallback(async () => {
    try {
      await stopHrStream();
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      setConnectionState((prev) => ({ ...prev, streaming: false }));
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
    hrData,
    currentHR,
    rrIntervals,
    error,
    scan,
    connect,
    disconnect,
    startStream,
    stopStream,
  };
}
