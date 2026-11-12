import { sampleAdaptersEnabled } from "../../lib/sampleAdapters";
import { makeSampleHemisync, unwiredHemisync, type HemisyncAdapter } from "./adapters";

/**
 * The adapter the Neuro screens use (decision 0010). Sample in mock mode; otherwise the live
 * adapter once the backend lands (until then: unwired, "Needs wiring").
 * Backend: replace the live branch with the real implementation; keep this export's name.
 */
let sampleSingleton: HemisyncAdapter | null = null;
export function hemisyncAdapter(): HemisyncAdapter {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleHemisync());
  return unwiredHemisync;
}
