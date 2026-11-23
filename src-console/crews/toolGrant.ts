import { UNWIRED, sample, sampleAdaptersEnabled, type Wired } from "../lib/sampleAdapters";

/**
 * design-1110i: the per-crew "Files and terminal" switch. Off by default, per crew, fail-closed.
 * Only Taisun grants; crews can only ask. The switch shows On only when crew-service reports the
 * allowlist it is enforcing (`enforced`), never from local state. Claude wires `liveCrewTools`.
 */
export type GrantDuration = "run" | "1h" | "24h";
export const DEFAULT_DURATION: GrantDuration = "1h";
export const DURATION_LABEL: Record<GrantDuration, string> = { run: "This run", "1h": "1 hour", "24h": "24 hours" };
export const DURATION_ACTION: Record<GrantDuration, string> = { run: "Turn On for This Run", "1h": "Turn On for 1 Hour", "24h": "Turn On for 24 Hours" };
/** A crew's request expires after this long if Taisun does nothing (README decision 4). */
export const REQUEST_EXPIRY_MIN = 60;
/** crew-service must confirm within this long, or the switch stays off and shows the error. */
export const CONFIRM_TIMEOUT_S = 15;
/** Never allowed for crews, switch on or off. */
export const ALWAYS_BLOCKED = ["Outbound sends", "spending", "git push", "sudo"] as const;

export type GrantRequest = { role: string; task: string; reason: string; requestedAt: string; expiresAt: string; duration: GrantDuration; scopePath: string };
export type CrewToolGrant = {
  crew: string;
  status: "off" | "pending" | "applying" | "on" | "error";
  /** This run's folder: the only place file and terminal tools may act. */
  scopePath: string;
  duration?: GrantDuration;
  grantedAt?: string;
  expiresAt?: string;
  /** From crew-service, not the app. Present only when status is "on". */
  enforced?: string[];
  request?: GrantRequest;
  error?: string;
};
export type AuditRow = { id: string; at: string; actor: string; kind: "grant" | "revoke" | "confirm" | "shell" | "fs_write" | "blocked" | "asked" | "info"; text: string; target?: string };

export interface CrewToolsAdapter {
  grant(crew: string): Wired<CrewToolGrant>;
  audit(crew: string): Wired<AuditRow[]>;
  /** Taisun's click only. Resolves once crew-service confirms (or fails / times out → error, stays off). */
  turnOn(crew: string, duration: GrantDuration): Promise<Wired<CrewToolGrant>>;
  turnOff(crew: string): Promise<Wired<CrewToolGrant>>;
  /** Approve or deny a crew's request (approve = turnOn with the requested duration). */
  decide(crew: string, approve: boolean): Promise<Wired<CrewToolGrant>>;
}

export const unwiredCrewTools: CrewToolsAdapter = {
  grant: () => UNWIRED, audit: () => UNWIRED,
  turnOn: async () => UNWIRED, turnOff: async () => UNWIRED, decide: async () => UNWIRED,
};

const slug = (crew: string) => crew.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "crew";
const clock = (d: Date) => d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit", timeZone: "Asia/Tokyo" });

/** Preview/test only: the mockup's states, tagged Sample data. "sample research" has asked; everything else starts off. */
export function makeSampleCrewTools(now: () => Date = () => new Date(), confirmMs = 1200): CrewToolsAdapter {
  const grants = new Map<string, CrewToolGrant>();
  const audits = new Map<string, AuditRow[]>();
  let n = 0;
  const log = (crew: string, row: Omit<AuditRow, "id" | "at"> & { at?: string }) => { audits.set(crew, [{ id: `a${++n}`, at: row.at ?? clock(now()), ...row }, ...(audits.get(crew) ?? [])]); };
  const base = (crew: string): CrewToolGrant => {
    const got = grants.get(crew);
    if (got) return got;
    const scopePath = `~/crew-sandbox/${slug(crew)}/latest-run`;
    const g: CrewToolGrant = crew === "sample research"
      ? { crew, status: "pending", scopePath, request: { role: "executor", task: "Run the test suite and write findings to notes/", reason: "the task's verification step runs npm test", requestedAt: "12:38 PM", expiresAt: "1:38 PM", duration: "1h", scopePath } }
      : { crew, status: "off", scopePath };
    grants.set(crew, g);
    audits.set(crew, crew === "sample research"
      ? [{ id: `a${++n}`, at: "12:38 PM", actor: "executor", kind: "asked", text: "Asked for files and terminal · 1 hour", target: scopePath }, { id: `a${++n}`, at: "Oct 21, 9:02 AM", actor: "system", kind: "info", text: "Crew created read-only (default)" }]
      : [{ id: `a${++n}`, at: "Oct 21, 9:02 AM", actor: "system", kind: "info", text: "Crew created read-only (default)" }]);
    return g;
  };
  const set = (g: CrewToolGrant) => { grants.set(g.crew, g); return sample(g); };
  const confirm = (crew: string, duration: GrantDuration) => new Promise<Wired<CrewToolGrant>>((resolve) => {
    window.setTimeout(() => {
      const t = now();
      const end = duration === "run" ? undefined : new Date(t.getTime() + (duration === "1h" ? 60 : 24 * 60) * 60_000);
      const enforced = ["kb_query", "memory_query", "fs_read", "fs_write", "shell"];
      log(crew, { actor: "crew-service", kind: "confirm", text: "Backend confirmed allowlist", target: enforced.join(", ") });
      resolve(set({ ...base(crew), status: "on", duration, grantedAt: clock(t), expiresAt: end ? clock(end) : undefined, enforced, request: undefined }));
    }, confirmMs);
  });
  return {
    grant: (crew) => sample(base(crew)),
    audit: (crew) => { base(crew); return sample(audits.get(crew) ?? []); },
    turnOn: (crew, duration) => {
      const g = base(crew);
      set({ ...g, status: "applying", duration });
      log(crew, { actor: "Taisun", kind: "grant", text: `You turned on files and terminal · ${DURATION_LABEL[duration]}`, target: g.scopePath });
      return confirm(crew, duration);
    },
    turnOff: async (crew) => {
      const g = base(crew);
      log(crew, { actor: "Taisun", kind: "revoke", text: "You turned off files and terminal · any running command was stopped" });
      return set({ crew, status: "off", scopePath: g.scopePath });
    },
    decide: async (crew, approve) => {
      const g = base(crew);
      if (!approve) {
        log(crew, { actor: "Taisun", kind: "revoke", text: "You denied the request · stays off" });
        return set({ crew, status: "off", scopePath: g.scopePath });
      }
      const duration = g.request?.duration ?? DEFAULT_DURATION;
      set({ ...g, status: "applying", duration });
      log(crew, { actor: "Taisun", kind: "grant", text: `You approved the request · ${DURATION_LABEL[duration]}`, target: g.scopePath });
      return confirm(crew, duration);
    },
  };
}

let sampleSingleton: CrewToolsAdapter | null = null;
export const crewToolsAdapter = (): CrewToolsAdapter => (sampleAdaptersEnabled() ? (sampleSingleton ??= makeSampleCrewTools()) : unwiredCrewTools);
