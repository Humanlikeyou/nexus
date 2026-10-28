import { useSyncExternalStore } from "react";
import { museTelemetry, type MuseTelemetry } from "./telemetryStore";

export type { MuseTelemetry } from "./telemetryStore";

/**
 * Subscribe to live Muse telemetry. Only call this in components that render
 * live readings (the Neuro/Live screen); the store is started by useMuseSession.
 */
export function useMuseTelemetry(): MuseTelemetry {
  return useSyncExternalStore(museTelemetry.subscribe, museTelemetry.getSnapshot, museTelemetry.getSnapshot);
}
