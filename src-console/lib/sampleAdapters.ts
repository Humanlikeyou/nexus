/**
 * design-1110d: panels with no backend yet read from adapters. In the app they are unwired (the
 * panel says "Needs wiring"); sample adapters are opt-in for previews and tests only, and every
 * value they return is shown with a visible "Sample data" tag.
 */
export const SAMPLE_ADAPTERS_KEY = "nexus.sampleAdapters";

export function sampleAdaptersEnabled(): boolean {
  try { return globalThis.localStorage?.getItem(SAMPLE_ADAPTERS_KEY) === "1"; } catch { return false; }
}

/** What an adapter knows: nothing yet, sample values (labelled), or live values from a real source. */
export type Wired<T> = { state: "unwired"; reason?: "currency" } | { state: "sample"; value: T } | { state: "live"; value: T };

export const UNWIRED = { state: "unwired" } as const;
/** USD only (Taisun, 15:24 JST): yen ledger values wait for a conversion source instead of a made-up USD figure. */
export const NEEDS_CONVERSION = { state: "unwired", reason: "currency" } as const;
export const sample = <T,>(value: T): Wired<T> => ({ state: "sample", value });
export const valueOf = <T,>(w: Wired<T>): T | null => (w.state === "unwired" ? null : w.value);
