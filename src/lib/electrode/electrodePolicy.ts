import type { ElectrodeMode, DataSource } from "@/lib/types";

/**
 * Derive the safe electrode operating mode based on current system state.
 *
 * Safety rules (in priority order):
 * 1. Output is ALWAYS locked off — "output" mode is never returned.
 *    This will remain until Stage 2+ safety review + confirmed Myndlift API constraints.
 * 2. During replay or demo → force "off" (no real hardware interaction)
 * 3. If confidence is below threshold → force "off"
 * 4. If device is connected and streaming → "read"
 * 5. If device is connected but not streaming → "off"
 * 6. Default → "off"
 */
export function deriveElectrodeMode(
  connected: boolean,
  streaming: boolean,
  dataSource: DataSource,
  confidence: number,
): ElectrodeMode {
  // HARD LOCK: output is never allowed at this stage
  // Even if future callers request "output", this function never returns it.

  // Virtual sources → no real hardware
  if (dataSource === "replay" || dataSource === "demo") {
    return "off";
  }

  // Low confidence → don't trust electrode state
  if (confidence < 0.3) {
    return "off";
  }

  // Connected + streaming → passive read
  if (connected && streaming) {
    return "read";
  }

  return "off";
}

/**
 * Check whether output mode is safe to enable.
 * Currently ALWAYS returns false — output is locked off.
 */
export function isOutputAllowed(): boolean {
  return false;
}
