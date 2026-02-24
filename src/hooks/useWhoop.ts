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
const RECONNECT_DELAYS = [3000, 6000, 12000]; // exponential backoff (ms)

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
  const lastAddressRef = useRef<string | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const userDisconnectedRef = useRef(false);

  // Listen to Rust-emitted BLE state (single source of truth)
  useEffect(() => {
    let cancelled = false;
    listen<BleFlowState>("whoop-ble-state", (event) => {
      if (cancelled) return;
      const newState = event.payload;
      setBleState(newState);
      // Unexpected drop: was streaming/connected, now idle — attempt reconnect
      if (newState === "idle" && !userDisconnectedRef.current && lastAddressRef.current) {
        const attempt = reconnectAttemptRef.current;
        if (attempt < RECONNECT_DELAYS.length) {
          const delay = RECONNECT_DELAYS[attempt];
          reconnectAttemptRef.current = attempt + 1;
          if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
          reconnectTimerRef.current = setTimeout(async () => {
            if (!cancelled && lastAddressRef.current) {
              try {
                const state = await connectWhoop(lastAddressRef.current);
                setConnectionState(state);
                setConnecting(false);
                reconnectAttemptRef.current = 0;
              } catch {
                // next timeout will retry if attempts remain
              }
            }
          }, delay);
        }
      } else if (newState === "connected" || newState === "streaming") {
        reconnectAttemptRef.current = 0;
        if (reconnectTimerRef.current) {
          clearTimeout(reconnectTimerRef.current);
          reconnectTimerRef.current = null;
        }
      }
    }).then((unlisten) => {
      unlistenStateRef.current = unlisten;
    });
    return () => {
      cancelled = true;
      if (reconnectTimerRef.current) clearTimeout(reconnectTimerRef.current);
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
    userDisconnectedRef.current = false;
    try {
      const state = await connectWhoop(address);
      lastAddressRef.current = address;
      reconnectAttemptRef.current = 0;
      setConnectionState(state);
    } catch (e) {
      setError(String(e));
    } finally {
      setConnecting(false);
    }
  }, []);

  const disconnect = useCallback(async () => {
    userDisconnectedRef.current = true;
    lastAddressRef.current = null;
    reconnectAttemptRef.current = 0;
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
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
