import { useCallback, useEffect, useState } from "react";
import { getHypnogram, getRecentWorkouts, type Hypnogram, type NoopWorkout } from "../bridge";
import { noopQuery, parseMetricSeries } from "../lib/noop";
import { sampleAdaptersEnabled } from "../lib/sampleAdapters";
import type { NoopMetricKey, NoopMetricPoint } from "../types";
import { sampleHypnogram, sampleSeries, sampleWorkouts } from "./sampleData";

/**
 * 0017 adapters for the Fitness tabs. Same pattern as the other panels (design-1110d): with no
 * source the panel says "Needs wiring", sample values exist only behind nexus.sampleAdapters=1 and
 * are tagged "Sample data", and a failed read is a plain sentence, never the backend's text.
 */
export type Load<T> =
  | { state: "loading" }
  | { state: "unwired" }
  | { state: "error" }
  | { state: "empty" }
  | { state: "sample"; value: T }
  | { state: "live"; value: T };

export const LOADING = { state: "loading" } as const;

/** The health store hasn't been created on this Mac (noop.rs resolve) or the helper isn't set up. */
const NO_STORE = /has not created|not created its local|binary_missing|invalid_path|sandbox_missing|spawn_failed/i;
const text = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : e && typeof e === "object" ? `${String((e as { kind?: unknown }).kind ?? "")} ${String((e as { detail?: unknown }).detail ?? "")}` : "");
export const failureState = (e: unknown): "unwired" | "error" => (NO_STORE.test(text(e)) ? "unwired" : "error");

export async function loadWorkouts(days: number): Promise<Load<NoopWorkout[]>> {
  if (sampleAdaptersEnabled()) return { state: "sample", value: sampleWorkouts(days) };
  try {
    const rows = await getRecentWorkouts(days);
    if (!Array.isArray(rows)) return { state: "error" };
    return rows.length ? { state: "live", value: rows } : { state: "empty" };
  } catch (e) {
    return { state: failureState(e) };
  }
}

export async function loadHypnogram(day?: string): Promise<Load<Hypnogram>> {
  if (sampleAdaptersEnabled()) return { state: "sample", value: sampleHypnogram(day) };
  try {
    const h = await getHypnogram(day);
    return h && Array.isArray(h.segments) && h.segments.length ? { state: "live", value: h } : { state: "empty" };
  } catch (e) {
    return { state: failureState(e) };
  }
}

/** Every key the metric_series allowlist accepts (noop_query.rs METRIC_KEYS). */
export const METRIC_KEYS: readonly NoopMetricKey[] = ["recovery", "hrv", "rhr", "resp_rate", "spo2", "strain", "sleep_total_min", "steps", "active_kcal"];
export type SeriesRead = { status: "ready"; points: NoopMetricPoint[] } | { status: "empty" } | { status: "error" };
export type TrendsData = Record<NoopMetricKey, SeriesRead>;
/** One read of a year; the range control slices it on screen, so a range change costs nothing. */
export const TRENDS_DAYS = 365;
const INACTIVE = new Set(["binary_missing", "invalid_path", "sandbox_missing", "spawn_failed"]);

export async function loadTrends(): Promise<Load<TrendsData>> {
  if (sampleAdaptersEnabled()) return { state: "sample", value: sampleSeries(TRENDS_DAYS) };
  const reads = await Promise.all(METRIC_KEYS.map((key) => noopQuery("metric_series", { key, days: TRENDS_DAYS })));
  if (reads.every((r) => !r.ok && INACTIVE.has(r.error.kind))) return { state: "unwired" };
  const out = {} as TrendsData;
  METRIC_KEYS.forEach((key, i) => {
    const r = reads[i];
    if (!r.ok) { out[key] = { status: "error" }; return; }
    try {
      const points = parseMetricSeries(key, r.value).points;
      out[key] = points.some((p) => p.value != null) ? { status: "ready", points } : { status: "empty" };
    } catch { out[key] = { status: "error" }; }
  });
  const all = Object.values(out);
  if (all.every((s) => s.status === "error")) return { state: "error" };
  if (all.every((s) => s.status === "empty")) return { state: "empty" };
  return { state: "live", value: out };
}

/** Runs a loader on mount and whenever `key` (which names the loader's inputs) changes; stale answers are dropped. */
export function useLoad<T>(loader: () => Promise<Load<T>>, key: string): [Load<T>, () => void] {
  const [done, setDone] = useState<{ id: string; load: Load<T> } | null>(null);
  const [nonce, setNonce] = useState(0);
  const id = `${key}#${nonce}`;
  useEffect(() => {
    let live = true;
    loader().then((load) => { if (live) setDone({ id, load }); }, () => { if (live) setDone({ id, load: { state: "error" } }); });
    return () => { live = false; };
    // `id` identifies the loader's inputs; re-running on loader identity would refetch every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  return [done && done.id === id ? done.load : LOADING, useCallback(() => setNonce((n) => n + 1), [])];
}
