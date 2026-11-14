import { invokeBound } from "../bridge";
import { UNWIRED, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";
import { makeSampleSystemMap, unwiredSystemMap, type SystemMap, type SystemMapAdapter } from "./systemMap";

/**
 * The adapter the Services map uses (decision 0011). Sample in mock mode; live inside Tauri via
 * `system_map_status`; unwired elsewhere (tests, browser).
 */
let sampleSingleton: SystemMapAdapter | null = null;
let liveSingleton: SystemMapAdapter | null = null;

/** Live adapter: `map()` reads a cache filled by `refresh()`. One refresh in flight at a time. */
export function makeLiveSystemMap(fetchMap: () => Promise<SystemMap> = () => invokeBound<SystemMap>("system_map_status")): SystemMapAdapter {
  let cache: SystemMap | null = null;
  let inFlight: Promise<void> | null = null;
  return {
    map: (): Wired<SystemMap> => (cache ? { state: "live", value: cache } : UNWIRED),
    refresh: () => {
      inFlight ??= (async () => {
        try {
          const next = await fetchMap();
          if (next && Array.isArray(next.nodes) && Array.isArray(next.paths)) cache = next;
        } catch {
          // Keep the last good map; a failed refresh never marks anything ok.
        } finally {
          inFlight = null;
        }
      })();
      return inFlight;
    },
  };
}

export function systemMapAdapter(): SystemMapAdapter {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleSystemMap());
  const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return inTauri ? (liveSingleton ??= makeLiveSystemMap()) : unwiredSystemMap;
}
