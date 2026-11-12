import { useEffect, useMemo, useSyncExternalStore } from "react";
import { getN8nStatus } from "../bridge";
import type { N8nStatusRow } from "../types";
import { unwiredCrews, type CrewsAdapter } from "./adapters";

/** n8n_status (read-only probe) cache. The Crews adapter is synchronous, so the async result lands here. */
let cache: N8nStatusRow | null = null;
const listeners = new Set<() => void>();
const subscribe = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };
export const setN8n = (next: N8nStatusRow | null): void => { cache = next; listeners.forEach((fn) => fn()); };
export const getN8n = (): N8nStatusRow | null => cache;

const isCount = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v) && v >= 0;

/**
 * Live n8n() over `base`, only while base's n8n is still unwired. N8nStatus has numeric counts, so
 * it is wired only when n8n answered AND both counts are known (they need an API key file in app
 * data); otherwise it stays unwired and the tile says so. Nothing is invented.
 */
export function liveN8nAdapter(base: CrewsAdapter, data: N8nStatusRow | null): CrewsAdapter {
  if (base.n8n !== unwiredCrews.n8n || !data || !data.connected || !isCount(data.workflows) || !isCount(data.failures7d)) return base;
  const value = { connected: true, workflows: data.workflows, failures7d: data.failures7d };
  return { ...base, n8n: () => ({ state: "live", value }) };
}

export async function loadN8n(fetcher: typeof getN8nStatus = getN8nStatus): Promise<void> {
  try {
    const s = await fetcher();
    setN8n(s && typeof s.connected === "boolean" ? s : null);
  } catch {
    setN8n(null);
  }
}

/** Reads n8n_status every 30 s while the Crews view is mounted and the crew service is online. */
export function useLiveN8n(base: CrewsAdapter, online: boolean): CrewsAdapter {
  const data = useSyncExternalStore(subscribe, getN8n);
  const active = online && base.n8n === unwiredCrews.n8n;
  useEffect(() => {
    if (!active) return;
    void loadN8n();
    const t = setInterval(() => void loadN8n(), 30_000);
    return () => clearInterval(t);
  }, [active]);
  return useMemo(() => (active ? liveN8nAdapter(base, data) : base), [active, base, data]);
}
