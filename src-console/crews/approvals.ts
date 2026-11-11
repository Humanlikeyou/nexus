import { useEffect, useMemo, useSyncExternalStore } from "react";
import { listCrewApprovals } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { dateTime } from "../lib/format";
import type { CrewApprovalRow } from "../types";
import { unwiredCrews, type CrewApproval, type CrewDecision, type CrewsAdapter } from "./adapters";

/** Crew approvals only (crew_list_approvals). Terrance's own action ledger never
 *  passes through here; a row that is not a crew-service approval is dropped. */
export interface ApprovalsCache { pending: CrewApprovalRow[]; decided: CrewApprovalRow[] }

let cache: ApprovalsCache | null = null; // null = service down / not loaded: the panels stay unwired
const listeners = new Set<() => void>();
const subscribe = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };
const emit = () => listeners.forEach((fn) => fn());

export const setApprovals = (next: ApprovalsCache | null): void => { cache = next; emit(); };
export const getApprovals = (): ApprovalsCache | null => cache;

const KINDS = new Set(["write", "destructive", "skill", "retry_limit", "claude_tier"]);
export const isCrewApproval = (r: unknown): r is CrewApprovalRow => {
  const o = r as Partial<CrewApprovalRow> | null;
  return !!o && typeof o.id === "string" && typeof o.run_id === "string" && typeof o.crew === "string" && typeof o.kind === "string" && KINDS.has(o.kind) && typeof o.title === "string";
};

const IMPACT: Record<string, string> = {
  write: "Changes", destructive: "Destructive changes",
  skill: "Skill or instruction change", retry_limit: "Retry limit reached", claude_tier: "Claude tier",
};
const RISK = new Set(["Low", "Medium", "High"]);

export function ago(iso: string, now: number = Date.now()): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "";
  const m = Math.max(0, Math.floor((now - t) / 60_000));
  if (m < 1) return "just now";
  if (m < 60) return `${m} min ago`;
  if (m < 60 * 24) return `${Math.floor(m / 60)} h ago`;
  return `${Math.floor(m / (60 * 24))} d ago`;
}

export function mapPending(rows: CrewApprovalRow[], now?: number): CrewApproval[] {
  return rows.filter(isCrewApproval).map((r) => ({
    id: r.id, title: r.title, crew: r.crew, agents: r.agents.join(" → "),
    impact: [IMPACT[r.kind] ?? "Action", ...(r.items?.length ? [r.items.join(", ")] : [])].join(" \u00b7 "), requested: ago(r.requested_at, now),
    risk: (RISK.has(r.risk) ? r.risk : "Medium") as CrewApproval["risk"],
    reason: r.reason, amount: UNWIRED, // no money tools exist, so there is never an amount
  }));
}

export function mapDecided(rows: CrewApprovalRow[]): CrewDecision[] {
  return rows.filter(isCrewApproval).map((r) => ({
    id: r.id, title: r.title, crew: r.crew, when: r.decided_at ? dateTime(r.decided_at) : "", approved: r.approved === true,
  }));
}

/** Live approvals/decided over `base`. Applies only while base's approvals are still unwired (sample mode wins). */
export function liveApprovalsAdapter(base: CrewsAdapter, data: ApprovalsCache | null, now?: number): CrewsAdapter {
  if (base.approvals !== unwiredCrews.approvals || !data) return base;
  const live = <T,>(value: T): Wired<T> => ({ state: "live", value });
  return { ...base, approvals: () => live(mapPending(data.pending, now)), decided: () => live(mapDecided(data.decided)) };
}

export async function loadApprovals(fetcher: typeof listCrewApprovals = listCrewApprovals): Promise<void> {
  try {
    const [pending, decided] = await Promise.all([fetcher("pending"), fetcher("decided")]);
    if (!Array.isArray(pending) || !Array.isArray(decided)) throw new Error("unexpected reply");
    setApprovals({ pending, decided });
  } catch {
    setApprovals(null); // service down or an older service without the route
  }
}

/** Reads crew approvals every 5 s while the Crews view is mounted and the service is online. */
export function useLiveApprovals(base: CrewsAdapter, online: boolean): CrewsAdapter {
  const data = useSyncExternalStore(subscribe, getApprovals);
  const active = online && base.approvals === unwiredCrews.approvals;
  useEffect(() => {
    if (!active) { setApprovals(null); return; }
    void loadApprovals();
    const t = setInterval(() => void loadApprovals(), 5000);
    return () => clearInterval(t);
  }, [active]);
  return useMemo(() => (active ? liveApprovalsAdapter(base, data) : base), [active, base, data]);
}
