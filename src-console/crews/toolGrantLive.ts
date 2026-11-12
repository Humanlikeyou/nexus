import { useEffect, useMemo, useSyncExternalStore } from "react";
import { decideCrewApproval, getCrewToolAudit, getCrewToolGrant, revokeCrewToolGrant, setCrewToolGrant } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { dateTime } from "../lib/format";
import type { CrewAuditApiRow, CrewToolGrantRow } from "../types";
import { unwiredCrewTools, type AuditRow, type CrewToolGrant, type CrewToolsAdapter, type GrantDuration } from "./toolGrant";

/** What the switch turns on (1110i). Writes and commands still need the batch approval. */
export const GRANT_TOOLS = ["fs_read", "fs_write", "shell"];

const slug = (crew: string) => crew.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "crew";
const live = <T,>(value: T): Wired<T> => ({ state: "live", value });
const when = (epochS: number) => dateTime(new Date(epochS * 1000).toISOString());
const DURATIONS = new Set<string>(["run", "1h", "24h"]);
const KINDS = new Set(["grant", "revoke", "confirm", "shell", "fs_write", "blocked", "asked", "info"]);

/** The UI state comes only from what crew-service reports: "on" needs `enabled` and a non-empty enforced list. */
export function mapGrant(crew: string, row: CrewToolGrantRow): CrewToolGrant {
  const scopePath = row.scope_path ?? `~/crew-sandbox/${slug(crew)}`;
  const duration = row.duration && DURATIONS.has(row.duration) ? (row.duration as GrantDuration) : undefined;
  if (row.enabled && Array.isArray(row.enforced_allowlist) && row.enforced_allowlist.length > 0) {
    return { crew, status: "on", scopePath, duration, grantedAt: row.granted_at ? when(row.granted_at) : undefined, expiresAt: row.expires_at ? when(row.expires_at) : undefined, enforced: row.enforced_allowlist };
  }
  const r = row.pending_request;
  if (r) {
    const d = DURATIONS.has(r.duration) ? (r.duration as GrantDuration) : "run";
    return { crew, status: "pending", scopePath, request: { role: "a crew role", task: "", reason: r.reason || "it needs to read or change files", requestedAt: r.requested_at ? dateTime(r.requested_at) : "", expiresAt: r.expires_at ? dateTime(r.expires_at) : "", duration: d, scopePath: r.scope_path } };
  }
  return { crew, status: "off", scopePath };
}

export function mapAudit(rows: CrewAuditApiRow[]): AuditRow[] {
  return rows.map((r) => ({ id: String(r.id), at: when(r.at), actor: r.actor, kind: (KINDS.has(r.kind) ? r.kind : "info") as AuditRow["kind"], text: r.text, target: r.target ?? undefined }));
}

const grants = new Map<string, CrewToolGrant>();
const audits = new Map<string, AuditRow[]>();
const requestIds = new Map<string, string>(); // crew -> pending approval id (not part of the UI type)
const inFlight = new Set<string>();
const lastTry = new Map<string, number>(); // crew -> ms of the last read; a failed read is retried by the 5 s poll, not by render
const RETRY_MS = 5000;
const listeners = new Set<() => void>();
const subscribe = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };
let version = 0;
const emit = () => { version += 1; listeners.forEach((fn) => fn()); };

export async function refresh(crew: string): Promise<void> {
  if (inFlight.has(crew)) return;
  inFlight.add(crew);
  lastTry.set(crew, Date.now());
  try {
    const [g, a] = await Promise.all([getCrewToolGrant(crew), getCrewToolAudit(crew)]);
    grants.set(crew, mapGrant(crew, g));
    if (g.pending_request?.id) requestIds.set(crew, g.pending_request.id); else requestIds.delete(crew);
    audits.set(crew, Array.isArray(a) ? mapAudit(a) : []);
  } catch {
    grants.delete(crew); // service down or an older service: stays unwired, read-only stated
    audits.delete(crew);
  } finally {
    inFlight.delete(crew);
    emit();
  }
}

const failure = (crew: string, e: unknown): Wired<CrewToolGrant> => {
  const msg = typeof e === "string" ? e : "Could not turn on files and terminal.";
  const g: CrewToolGrant = { crew, status: "error", scopePath: grants.get(crew)?.scopePath ?? `~/crew-sandbox/${slug(crew)}`, error: msg.replace(/^declined:\s*/, "") };
  grants.set(crew, g);
  emit();
  return live(g);
};

export const liveCrewTools: CrewToolsAdapter = {
  grant: (crew) => {
    const g = grants.get(crew);
    if (!g && !inFlight.has(crew) && !lastTry.has(crew)) void refresh(crew); // render never retries a failed read
    return g ? live(g) : UNWIRED;
  },
  audit: (crew) => (audits.has(crew) ? live(audits.get(crew)!) : UNWIRED),
  turnOn: async (crew, duration) => {
    grants.set(crew, { ...(grants.get(crew) ?? { crew, scopePath: `~/crew-sandbox/${slug(crew)}` }), crew, status: "applying", duration } as CrewToolGrant);
    emit();
    try {
      const row = await setCrewToolGrant(crew, GRANT_TOOLS, duration);
      const g = mapGrant(crew, row);
      grants.set(crew, g.status === "on" ? g : { ...g, status: "error", error: "Crew service did not confirm. Files and terminal stay off." });
      void refresh(crew);
      return live(grants.get(crew)!);
    } catch (e) {
      void refresh(crew);
      return failure(crew, e);
    }
  },
  turnOff: async (crew) => {
    try {
      grants.set(crew, mapGrant(crew, await revokeCrewToolGrant(crew)));
    } catch (e) {
      return failure(crew, e);
    }
    void refresh(crew);
    return live(grants.get(crew)!);
  },
  decide: async (crew, approve) => {
    const id = requestIds.get(crew);
    if (!id) return failure(crew, "That request is no longer pending.");
    try {
      await decideCrewApproval(id, approve);
    } catch (e) {
      void refresh(crew);
      return failure(crew, e);
    }
    await refresh(crew);
    return live(grants.get(crew) ?? { crew, status: "off", scopePath: `~/crew-sandbox/${slug(crew)}` });
  },
};

/** Live tools adapter while `base` is the unwired one (sample mode wins) and the service is online. Re-reads every 5 s. */
export function useLiveCrewTools(base: CrewToolsAdapter, online: boolean): CrewToolsAdapter {
  useSyncExternalStore(subscribe, () => version);
  const active = online && base === unwiredCrewTools;
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => lastTry.forEach((_t, crew) => void refresh(crew)), RETRY_MS);
    return () => clearInterval(t);
  }, [active]);
  return useMemo(() => (active ? liveCrewTools : base), [active, base]);
}

export const clearCrewToolCache = (): void => { grants.clear(); audits.clear(); requestIds.clear(); lastTry.clear(); emit(); };
