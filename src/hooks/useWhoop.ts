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
const RECONNECT_DELAYS = [3000, 6000, 12000, 24000, 48000]; // backoff steps (ms)

/** Reconnect lifecycle visible to the UI. */
export type WhoopReconnectPhase = "idle" | "reconnecting" | "failed";

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
  const [reconnectPhase, setReconnectPhase] = useState<WhoopReconnectPhase>("idle");
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const unlistenStateRef = useRef<UnlistenFn | null>(null);
  const lastAddressRef = useRef<string | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const userDisconnectedRef = useRef(false);
  // Callbacks injected by consumers for reconnect lifecycle toasts
  const onReconnectingRef = useRef<(() => void) | null>(null);
  const onReconnectedRef = useRef<(() => void) | null>(null);
  const onReconnectFailedRef = useRef<(() => void) | null>(null);

  // Listen to Rust-emitted BLE state (single source of truth)
  useEffect(() => {
    let cancelled = false;

    listen<BleFlowState>("whoop-ble-state", (event) => {
      if (cancelled) return;
      const newState = event.payload;
      setBleState(newState);

      // Successful connection / reconnection
      if (newState === "connected" || newState === "streaming") {
        const wasReconnecting = reconnectAttemptRef.current > 0;
        reconnectAttemptRef.current = 0;
        if (reconnectTimerRef.current) {
          clearTimeout(reconnectTimerRef.current);
          reconnectTimerRef.current = null;
        }
        if (wasReconnecting) {
          setReconnectPhase("idle");
          onReconnectedRef.current?.();
        }
        return;
      }

      // Unexpected drop: was connected/streaming, now idle
      if (newState === "idle" && !userDisconnectedRef.current && lastAddressRef.current) {
        const attempt = reconnectAttemptRef.current;

        if (attempt === 0) {
          // First drop: notify UI once that reconnect is starting
          setReconnectPhase("reconnecting");
          onReconnectingRef.current?.();
        }

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
                // bleState event will fire "connected" and handle phase reset
              } catch {
                // next idle event will trigger the next attempt
              }
            }
          }, delay);
        } else {
          // All attempts exhausted
          setReconnectPhase("failed");
          onReconnectFailedRef.current?.();
          reconnectAttemptRef.current = 0;
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
    setReconnectPhase("idle");
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
    setReconnectPhase("idle");
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

  /** Manually retry after reconnect failed. Resets counters and tries immediately. */
  const retryConnect = useCallback(async () => {
    if (!lastAddressRef.current) return;
    setReconnectPhase("reconnecting");
    reconnectAttemptRef.current = 0;
    userDisconnectedRef.current = false;
    try {
      const state = await connectWhoop(lastAddressRef.current);
      setConnectionState(state);
      setReconnectPhase("idle");
    } catch (e) {
      setError(String(e));
      setReconnectPhase("failed");
    }
  }, []);

  /** Register lifecycle callbacks to fire toasts (call once from a component). */
  const setReconnectCallbacks = useCallback((
    onReconnecting: () => void,
    onReconnected: () => void,
    onFailed: () => void,
  ) => {
    onReconnectingRef.current = onReconnecting;
    onReconnectedRef.current = onReconnected;
    onReconnectFailedRef.current = onFailed;
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
    reconnectPhase,
    hrData,
    currentHR,
    rrIntervals,
    error,
    scan,
    connect,
    disconnect,
    retryConnect,
    setReconnectCallbacks,
    startStream,
    stopStream,
  };
}
