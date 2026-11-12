import { sampleAdaptersEnabled } from "../lib/sampleAdapters";
import { makeSampleSystemMap, unwiredSystemMap, type SystemMapAdapter } from "./systemMap";

/**
 * The adapter the Services map uses (decision 0011). Sample in mock mode; live once the backend's
 * `system_map_status` lands (backend: add the live branch here, keep this export's name).
 */
let sampleSingleton: SystemMapAdapter | null = null;
export function systemMapAdapter(): SystemMapAdapter {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleSystemMap());
  return unwiredSystemMap;
}
