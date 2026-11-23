import { useEffect, useMemo, useSyncExternalStore } from "react";
import { getCrewRunSteps } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import type { CrewInfo, CrewRunSteps } from "../types";
import { unwiredCrews, type CrewHandoff, type CrewRunMetrics, type CrewsAdapter, type CrewStep } from "./adapters";

/** Per-run step cache. Adapters are synchronous, so the async crew_run_steps result lands here
 *  and the live adapter reads it. */
const cache = new Map<string, CrewRunSteps>();
const listeners = new Set<() => void>();
const subscribe = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };

export function setRunDetail(id: string, detail: CrewRunSteps): void {
  cache.set(id, detail);
  listeners.forEach((fn) => fn());
}
export const getRunDetail = (id: string): CrewRunSteps | undefined => cache.get(id);
export const clearRunDetails = (): void => { cache.clear(); listeners.forEach((fn) => fn()); };

const inFlight = new Set<string>();

export async function loadRunDetail(id: string, fetcher: (id: string) => Promise<CrewRunSteps> = getCrewRunSteps): Promise<void> {
  if (inFlight.has(id)) return; // a slow fetch must not stack under the 3 s ticks
  inFlight.add(id);
  try {
    const d = await fetcher(id);
    setRunDetail(id, { steps: Array.isArray(d?.steps) ? d.steps : [], handoffs: Array.isArray(d?.handoffs) ? d.handoffs : [] });
  } catch {
    // Service down or old service without the route: keep whatever is cached; panels stay unwired.
  } finally {
    inFlight.delete(id);
  }
}

const fmtTook = (ms: number) => `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`;
const pct = (ms: number, total: number) => Math.min(100, Math.max(0, Math.round((ms / total) * 1000) / 10));

export function mapSteps(rows: CrewRunSteps["steps"]): CrewStep[] {
  const total = Math.max(1, ...rows.map((r) => r.end_ms));
  return rows.map((r) => ({
    id: String(r.id),
    label: r.label,
    agent: r.agent,
    startPct: pct(r.start_ms, total),
    endPct: pct(r.end_ms, total),
    status: r.status === "failed" || r.status === "skipped" ? r.status : "ok",
    took: r.status === "skipped" ? "skipped" : fmtTook(Math.max(0, r.end_ms - r.start_ms)),
  }));
}

export function mapMetrics(run: CrewInfo): CrewRunMetrics | null {
  const start = run.started_at ? Date.parse(run.started_at) : NaN;
  const end = run.finished_at ? Date.parse(run.finished_at) : NaN;
  if (!Number.isFinite(start) || !Number.isFinite(end)) return null;
  if (run.tokens == null || run.requests == null || run.cost_usd == null) return null;
  return { durationS: Math.max(0, Math.round((end - start) / 1000)), tokens: run.tokens, requests: run.requests, costUsd: run.cost_usd };
}

/** Live values for one run over `base`; anything without data stays unwired (never invented). */
export function liveRunAdapter(base: CrewsAdapter, run: CrewInfo | null, detail: CrewRunSteps | undefined): CrewsAdapter {
  if (base !== unwiredCrews || !run) return base;
  const live = <T,>(v: T | null): Wired<T> => (v === null ? UNWIRED : { state: "live", value: v });
  return {
    ...base,
    runMetrics: (id) => (id === run.id ? live(mapMetrics(run)) : UNWIRED),
    steps: (id) => (id === run.id && detail?.steps.length ? live(mapSteps(detail.steps)) : UNWIRED),
    handoffs: (id): Wired<CrewHandoff[]> => (id === run.id && detail?.handoffs.length ? live(detail.handoffs.map((h) => ({ from: h.from, to: h.to }))) : UNWIRED),
  };
}

/** Loads the viewed run's steps (and re-reads every 3 s while it runs); returns the adapter to use. */
export function useLiveRunAdapter(base: CrewsAdapter, run: CrewInfo | null): CrewsAdapter {
  const id = run?.id ?? null;
  const status = run?.status;
  const detail = useSyncExternalStore(subscribe, () => (id ? cache.get(id) : undefined));
  useEffect(() => {
    if (!id || base !== unwiredCrews) return;
    void loadRunDetail(id);
    if (status !== "running") return;
    const t = setInterval(() => void loadRunDetail(id), 3000);
    return () => clearInterval(t);
  }, [id, status, base]);
  return useMemo(() => liveRunAdapter(base, run, detail), [base, run, detail]);
}
