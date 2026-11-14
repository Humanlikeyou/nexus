import { getServiceEvents, getServicesStatus, getServicesSummary, invokeBound } from "../bridge";
import { listen } from "@tauri-apps/api/event";
import type { AlertKind, BackupRun, ModelTier, SetupDownload, BackupSettings, BackupStatus, KeyFileStatus, RelayPreview, RelaySettings, RestorePreview, SecretName, SecretStatus, ServiceEvent, ServiceLogs, ServicesSummary, StandaloneAdapter, SystemProfile, WhoopStatus } from "./adapters";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { servicesFromMap } from "./servicesFromMap";
import { eventsFromSupervisor, isSupStatus, serviceFromSupervisor, servicesFromSupervisor } from "./servicesFromSupervisor";
import { summaryFromServices, summaryFromSupervisor } from "./servicesSummary";
import type { ModelTier_Serialize, ServiceId, ServiceStatus, StartAllRow } from "../bindings.gen";
import type { SystemMap } from "./systemMap";

/**
 * Live standalone adapter (HANDOFF-1111s wiring steps 1-2): secrets and the n8n key file, over
 * Tauri commands. Adapters are synchronous over already-loaded data, so this keeps a small cache that
 * `refresh()` fills; views re-render through `subscribeLive`. Everything not wired yet spreads the
 * unwired base. No value ever crosses back: the commands return `set` flags and status only, and
 * errors are logged by code, never with the value or passphrase.
 */
/** Row shape of `backup_list` (src-tauri backups.rs). `sizeBytes` is not shown by the UI yet. */
type BackupRow = { id: string; label: string; madeAt: string; sizeBytes: number; daysOld: number };
/** Result of `backup_verify_last` (null = no backup). */
export type VerifyRow = { file: string; at: string; size: number; ok: boolean; checked: number; mismatched: string[]; mode: string };
/** `whoop_status` (src-tauri whoop_status.rs): read-only owner + freshness. Counts and times only. */
export type WhoopRow = { owner: "amara" | "legacy" | "none"; storeFound: boolean; lastSampleAt: number | null; gaps14d: number | null };
/** `system_profile` (src-tauri system_profile.rs). No serial number, hostname or user name exists in it. */
export type ProfileRow = { ramGb: number; chip: string; arch: string; cores: number; macos: string | null; fileVault: boolean | null };
type Cache = {
  secrets: SecretStatus[] | null;
  keyFile: KeyFileStatus | null;
  backupSettings: BackupSettings | null;
  backupList: BackupRow[] | null;
  verify: VerifyRow | null;
  run: BackupRun | null;
  map: SystemMap | null;
  relay: RelaySettings | null;
  preview: RelayPreview | null;
  whoop: WhoopRow | null;
  profile: ProfileRow | null;
  /** Phase 8 supervisor snapshot; null when the command is missing or answers badly (older backend). */
  sup: ServiceStatus[] | null;
  supSummary: ServicesSummary | null;
  tiers: ModelTier[] | null;
  setupDone: boolean | null;
  /** Latest `model://progress` per role (monotonic within a run). */
  models: Record<string, ModelProg>;
  startRows: StartAllRow[] | null;
};
const cache: Cache = { secrets: null, keyFile: null, backupSettings: null, backupList: null, verify: null, run: null, map: null, relay: null, preview: null, whoop: null, profile: null, sup: null, supSummary: null, tiers: null, setupDone: null, models: {}, startRows: null };
let version = 0;
const listeners = new Set<() => void>();
const notify = () => { version += 1; listeners.forEach((l) => l()); };

export const subscribeLive = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };
export const liveVersion = () => version;

/** `model://progress` payload (src-tauri model_download.rs): no URL, bytes only. */
export type ModelProg = { state: "waiting" | "downloading" | "verifying" | "done" | "failed"; pct: number; bytesDone: number; bytesTotal: number };
const MODEL_STATES = ["waiting", "downloading", "verifying", "done", "failed"];
const live = <T,>(v: T | null): Wired<T> => (v === null ? UNWIRED : { state: "live", value: v });
const OK: Wired<{ ok: true }> = { state: "live", value: { ok: true } };
const code = (e: unknown) => (typeof e === "string" ? e.slice(0, 40) : "failed");

const isSettings = (v: unknown): v is BackupSettings => {
  const o = v as Record<string, unknown> | null;
  return !!o && typeof o.nightly === "boolean" && typeof o.at === "string" && typeof o.dir === "string" && typeof o.keep === "number"
    && typeof o.beforeUpdate === "boolean" && typeof o.skipOnBatteryBelow === "number";
};
const isRow = (r: unknown): r is BackupRow => {
  const o = r as Record<string, unknown> | null;
  return !!o && typeof o.id === "string" && typeof o.label === "string" && typeof o.madeAt === "string" && typeof o.daysOld === "number";
};
const fmtMadeAt = (iso: string) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString("en-US", { month: "short", day: "numeric", year: "numeric", hour: "numeric", minute: "2-digit" });
};
const isVerify = (v: unknown): v is VerifyRow => {
  const o = v as Record<string, unknown> | null;
  return !!o && typeof o.file === "string" && typeof o.ok === "boolean" && Array.isArray(o.mismatched) && o.mismatched.every((m) => typeof m === "string")
    && typeof o.size === "number" && typeof o.checked === "number" && typeof o.mode === "string";
};
const isWhoop = (v: unknown): v is WhoopRow => {
  const o = v as Record<string, unknown> | null;
  return !!o && (o.owner === "amara" || o.owner === "legacy" || o.owner === "none") && typeof o.storeFound === "boolean"
    && (o.lastSampleAt === null || typeof o.lastSampleAt === "number") && (o.gaps14d === null || typeof o.gaps14d === "number");
};
const isProfile = (v: unknown): v is ProfileRow => {
  const o = v as Record<string, unknown> | null;
  return !!o && typeof o.ramGb === "number" && o.ramGb > 0 && typeof o.chip === "string" && o.chip !== "" && typeof o.arch === "string"
    && typeof o.cores === "number" && (o.macos === null || typeof o.macos === "string") && (o.fileVault === null || typeof o.fileVault === "boolean");
};
const fmtSize = (n: number) => {
  if (!Number.isFinite(n) || n < 0) return "";
  const u = ["B", "KB", "MB", "GB", "TB"]; let i = 0; let v = n;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i += 1; }
  return `${i === 0 ? v : v.toFixed(v >= 100 ? 0 : 1)} ${u[i]}`;
};
/** Newest `backup_list` row (time, size) plus the verify result when it is for that same file. Null with no backups. */
export function mapLast(list: BackupRow[], verify: VerifyRow | null): BackupStatus["last"] {
  // Newest by madeAt, not by position: the order of backup_list is not relied on.
  const stamp = (r: BackupRow) => { const t = new Date(r.madeAt).getTime(); return Number.isNaN(t) ? -Infinity : t; };
  const newest = [...list].sort((a, b) => stamp(b) - stamp(a))[0];
  if (!newest) return null;
  const v = verify && verify.file === newest.id ? verify : null;
  return { at: fmtMadeAt(newest.madeAt), size: fmtSize(newest.sizeBytes), checksumOk: v ? v.ok : null, checks: v ? (v.mode === "sha256" ? "sha256" : "structure") : undefined };
}

/**
 * `backup_verify_last` reads the whole newest archive, so it is heavy: it runs once on startup only, never
 * from refreshLive or after a setting, key or backup change. One run at a time, cached per file+size.
 * A refusal (backup running, already busy) or any error leaves the checksum "not checked yet", never "failed".
 */
let verifyInflight: Promise<void> | null = null;
let verifiedKey: string | null = null;
export function verifyLast(call: typeof invokeBound = invokeBound): Promise<void> {
  const row = cache.backupList?.[0];
  const key = row ? `${row.id}:${row.sizeBytes}` : null;
  if (key && key === verifiedKey) return Promise.resolve();
  verifyInflight ??= (async () => {
    try {
      const v = await call<VerifyRow | null>("backup_verify_last");
      cache.verify = isVerify(v) ? v : null;
      verifiedKey = key;
    } catch { cache.verify = null; } finally { verifyInflight = null; notify(); }
  })();
  return verifyInflight;
}
export const resetVerifyForTests = () => { verifyInflight = null; verifiedKey = null; cache.verify = null; };

/**
 * `whoop_status` to the Devices card. Only what the backend knows: owner, newest sample time, gap count.
 * Amara has no strap connection yet (takeover is a later stage), so `connected` is false and live heart rate
 * is "delayed" at best (read from the health store). Nothing is invented: battery stays null (the panel falls
 * back to the health snapshot), and an unreadable store (gaps unknown) is unwired rather than "0 gaps".
 */
export function toWhoop(w: WhoopRow | null, now: number = Date.now()): Wired<WhoopStatus> {
  if (!w || w.gaps14d === null) return UNWIRED;
  const ageMin = w.lastSampleAt === null ? null : Math.max(0, Math.round((now / 1000 - w.lastSampleAt) / 60));
  const lastSync = ageMin === null ? "No reading yet" : ageMin < 1 ? "Last write just now" : ageMin < 120 ? `Last write ${ageMin} min ago` : `Last write ${fmtMadeAt(new Date(w.lastSampleAt! * 1000).toISOString())}`;
  return { state: "live", value: {
    owner: w.owner, connected: false,
    batteryPct: null, batteryNote: "from the health store",
    lastSync, lastSyncOk: w.lastSampleAt !== null,
    liveHr: w.owner === "legacy" && ageMin !== null && ageMin < 5 ? "delayed" : "off", liveNote: "Read from the health store, not from the strap",
    gaps14d: w.gaps14d,
    scores: "Read from the health store",
    store: { where: w.owner === "amara" ? "Amara\u2019s folder on this Mac" : "Current store · Amara reads it, read-only", verified: false, oldCopyUntil: null },
    lucid: false, muse: "In-app Bluetooth",
  } };
}
/** Needs a FileVault answer too: an unknown state is unwired, never "Off". */
export function toProfile(p: ProfileRow | null): Wired<SystemProfile> {
  return !p || p.fileVault === null ? UNWIRED : { state: "live", value: { ramGb: p.ramGb, chip: p.chip, fileVault: p.fileVault } };
}
/** `service_logs` tail (redacted by the backend) plus the supervisor's fixed-text events. Neither is made up; both missing is unwired. */
export function toLogs(lines: unknown, events: ServiceEvent[] | null = null): Wired<ServiceLogs> {
  const tail = Array.isArray(lines) && lines.every((l) => typeof l === "string") ? (lines as string[]) : null;
  return tail === null && (events === null || events.length === 0) ? UNWIRED : { state: "live", value: { events: events ?? [], tail: tail ?? [] } };
}
/** `restore_preview` row to the First-run preview. Contents are the archive's folders with file counts and sizes. */
export function toRestorePreview(v: unknown, row: BackupRow, keyFileMade: boolean): Wired<RestorePreview> {
  const o = v as Record<string, unknown> | null;
  const verify = o?.verify as Record<string, unknown> | undefined;
  const groups = o?.groups as unknown;
  if (!o || typeof o.file !== "string" || typeof o.bytes !== "number" || !verify || typeof verify.ok !== "boolean" || !Array.isArray(groups)) return UNWIRED;
  const contents = groups.flatMap((g) => {
    const r = g as Record<string, unknown> | null;
    return r && typeof r.name === "string" && typeof r.files === "number" && typeof r.bytes === "number" ? [{ title: r.name, sub: `${r.files} file${r.files === 1 ? "" : "s"} · ${fmtSize(r.bytes)}` }] : [];
  });
  return { state: "live", value: { file: o.file, madeOn: fmtMadeAt(row.madeAt), version: "", size: fmtSize(row.sizeBytes), checksumOk: verify.ok, contents, keyFileFound: keyFileMade } };
}
const newestRow = (list: BackupRow[] | null): BackupRow | null => {
  const t = (r: BackupRow) => { const n = new Date(r.madeAt).getTime(); return Number.isNaN(n) ? -Infinity : n; };
  return list && list.length ? [...list].sort((a, b) => t(b) - t(a))[0] : null;
};

/** Logs and the restore preview read whole files / run docker, so they load on first use only, never from refreshLive. */
const LOGS_FRESH_MS = 15_000;
const logsCache = new Map<string, { at: number; value: Wired<ServiceLogs> }>();
const logsInflight = new Set<string>();
export function askLogs(id: string, call: typeof invokeBound = invokeBound): Wired<ServiceLogs> {
  const hit = logsCache.get(id);
  if (!hit || Date.now() - hit.at > LOGS_FRESH_MS) {
    if (!logsInflight.has(id)) {
      logsInflight.add(id);
      const tail = call<{ lines?: unknown }>("service_logs", { id }).then((r) => r?.lines, () => null);
      const events = (call === invokeBound ? getServiceEvents(null, id as ServiceId) : call<unknown>("service_events", { since: null, id }))
        .then((r) => (Array.isArray(r) ? eventsFromSupervisor(r) : null), () => null);
      void Promise.all([tail, events]).then(([t, e]) => toLogs(t, e)).then((value) => { logsCache.set(id, { at: Date.now(), value }); }).finally(() => { logsInflight.delete(id); notify(); });
    }
  }
  return hit?.value ?? UNWIRED;
}
let previewKey: string | null = null;
let previewValue: Wired<RestorePreview> = UNWIRED;
let previewInflight = false;
export function askRestorePreview(call: typeof invokeBound = invokeBound): Wired<RestorePreview> {
  const row = newestRow(cache.backupList);
  if (!row) return UNWIRED;
  const key = `${row.id}:${row.sizeBytes}`;
  if (previewKey !== key && !previewInflight) {
    previewInflight = true;
    void call<unknown>("restore_preview", { file: row.id }).then((r) => toRestorePreview(r, row, cache.keyFile?.made === true), () => UNWIRED).then((v) => { previewValue = v; previewKey = key; }).finally(() => { previewInflight = false; notify(); });
  }
  return previewKey === key ? previewValue : UNWIRED;
}
export const resetOpsForTests = () => { logsCache.clear(); logsInflight.clear(); previewKey = null; previewValue = UNWIRED; previewInflight = false; cache.whoop = null; cache.profile = null; };

const ALERT_IDS: AlertKind[] = ["sleep", "recovery", "effort", "battery", "sync", "lucid", "illness"];
/** `notify_settings_get` / `_set` row: all seven kinds as booleans, or null (never trust the shape). */
export function toRelay(v: unknown): RelaySettings | null {
  const o = v as Record<string, unknown> | null;
  const kinds = o?.kinds as Record<string, unknown> | undefined;
  if (!o || typeof o.enabled !== "boolean" || !kinds || !ALERT_IDS.every((k) => typeof kinds[k] === "boolean")) return null;
  const nc = Array.isArray(o.notConnected) ? (o.notConnected.filter((k) => ALERT_IDS.includes(k as AlertKind)) as AlertKind[]) : [];
  return { enabled: o.enabled, kinds: Object.fromEntries(ALERT_IDS.map((k) => [k, kinds[k] as boolean])) as Record<AlertKind, boolean>, lastSent: typeof o.lastSent === "string" ? o.lastSent : null, notConnected: nc };
}
/** `notify_preview` row. The text is shown as written; the backend already checked its numbers. */
export function toPreview(v: unknown): RelayPreview | null {
  const o = v as Record<string, unknown> | null;
  return o && typeof o.text === "string" && typeof o.at === "string" && typeof o.from === "string" ? { text: o.text, at: o.at, from: o.from } : null;
}
/** The preview asks the local model, so it runs once on first use, never from refreshLive. */
let previewAsked = false;
function askPreview() {
  if (previewAsked) return;
  previewAsked = true;
  void invokeBound("notify_preview", { kind: "recovery" }).then((p) => { cache.preview = toPreview(p); }, () => { cache.preview = null; }).finally(notify);
}
export const resetRelayForTests = () => { previewAsked = false; cache.preview = null; cache.relay = null; };

/**
 * `model_tiers` rows to the picker. `note` null becomes undefined; a malformed row makes the whole list
 * unwired (never a partial or invented list). An empty list is unwired too.
 */
export function toTiers(v: unknown): ModelTier[] | null {
  if (!Array.isArray(v) || v.length === 0) return null;
  const out: ModelTier[] = [];
  for (const r of v as Array<Partial<ModelTier_Serialize> | null>) {
    if (!r || typeof r.id !== "string" || typeof r.name !== "string" || typeof r.minRamGb !== "number" || !Array.isArray(r.models) || !r.models.every((m) => typeof m === "string")
      || typeof r.download !== "string" || typeof r.recommended !== "boolean" || typeof r.available !== "boolean" || (r.note != null && typeof r.note !== "string")) return null;
    out.push({ id: r.id, name: r.name, minRamGb: r.minRamGb, models: r.models, download: r.download, note: r.note ?? undefined, recommended: r.recommended, available: r.available });
  }
  return out;
}

/**
 * Merge one progress event into the per-role map. Within a run pct and bytes never go back and a finished
 * role stays done; "waiting" starts a new run for that role. Malformed events are ignored.
 */
export function mergeProgress(prev: Record<string, ModelProg>, payload: unknown): Record<string, ModelProg> {
  const o = payload as Record<string, unknown> | null;
  if (!o || typeof o.role !== "string" || typeof o.state !== "string" || !MODEL_STATES.includes(o.state)
    || typeof o.pct !== "number" || typeof o.bytesDone !== "number" || typeof o.bytesTotal !== "number") return prev;
  const next: ModelProg = { state: o.state as ModelProg["state"], pct: Math.max(0, Math.min(100, Math.round(o.pct))), bytesDone: Math.max(0, o.bytesDone), bytesTotal: Math.max(0, o.bytesTotal) };
  const old = prev[o.role];
  if (old && next.state !== "waiting") {
    if (old.state === "done") return prev;
    if (next.state === "failed") return { ...prev, [o.role]: { ...old, state: "failed" } };
    next.pct = Math.max(old.pct, next.pct);
    next.bytesDone = Math.max(old.bytesDone, next.bytesDone);
  }
  return { ...prev, [o.role]: next };
}

const ROLE_NAME: Record<string, string> = { fast: "Fast model", deep: "Deep model", embed: "Embedding model" };
/**
 * Setup download view from real events and the supervisor only. Models: the roles seen in `model://progress`
 * (a failed role shows as waiting with the detail "Download failed": the type has no failed state, see
 * design-questions). Services: only the rows `services_start_all` reported once it has run, else every
 * supervisor row; a row `adopted`/`started` by start-all stands in for a supervisor state that has not
 * caught up yet. Nothing here is estimated: no time left, no counts beyond the bytes the backend sent.
 */
export function toSetupDownload(models: Record<string, ModelProg>, sup: ServiceStatus[] | null, rows: StartAllRow[] | null): Wired<SetupDownload> {
  const roles = Object.keys(models);
  if (roles.length === 0 && !sup) return UNWIRED;
  const detail = (m: ModelProg) => (m.state === "failed" ? "Download failed" : m.bytesTotal > 0 ? `${fmtSize(m.bytesDone)} of ${fmtSize(m.bytesTotal)}` : "");
  const modelRows = roles.map((r) => {
    const m = models[r];
    return { name: ROLE_NAME[r] ?? r, state: m.state === "done" ? "done" as const : m.state === "downloading" || m.state === "verifying" ? "downloading" as const : "waiting" as const, detail: detail(m), pct: m.state === "done" ? 100 : m.pct };
  });
  const byId = new Map((rows ?? []).map((r) => [r.id, r.outcome]));
  const services = (sup ?? []).filter((s) => !rows || byId.has(s.id)).map((s) => {
    const o = byId.get(s.id);
    const caughtUp = s.state === "healthy" || s.state === "degraded" || s.state === "separate";
    const state = caughtUp || ((o === "adopted" || o === "started") && (s.state === "unknown" || s.state === "starting")) ? "running" as const
      : s.state === "starting" || s.state === "restarting" ? "starting" as const : "waiting" as const;
    return { name: serviceFromSupervisor(s).name, state };
  });
  return { state: "live", value: { models: modelRows, services } };
}

/** Fixed codes the Rust setup commands return; anything else is logged as "failed". */
const SETUP_CODES = new Set(["declined", "busy", "approval_failed", "outbound_blocked", "host_not_allowed", "not_published", "manifest_invalid", "tier_not_found", "profile_unavailable", "storage_error", "checksum_mismatch", "upstream_error", "timed_out", "too_large", "unsafe_path", "no_space", "io_error", "invalid_tier", "setup_not_ready"]);
/** Setup may be marked done only after a live download of this tier resolved and start-all reported no failed, needs_you or binary_missing row. */
export function setupReady(downloadedTier: string | null, tier: string, rows: StartAllRow[] | null): boolean {
  return downloadedTier !== null && downloadedTier === tier && !!rows && rows.length > 0 && !rows.some((r) => r.outcome === "failed" || r.outcome === "needs_you" || r.outcome === "binary_missing");
}
let downloadedTier: string | null = null;
const setupCode = (e: unknown) => (typeof e === "string" && SETUP_CODES.has(e) ? e : "failed");

export async function refreshLive(): Promise<void> {
  const [s, k, bs, bl, sm, rl, wh, pr, sv, ss, td, st] = await Promise.allSettled([
    invokeBound<SecretStatus[]>("secrets_status"),
    invokeBound<KeyFileStatus>("key_file_status"),
    invokeBound<BackupSettings>("backup_settings_get"),
    invokeBound<BackupRow[]>("backup_list"),
    invokeBound<SystemMap>("system_map_status"),
    invokeBound<unknown>("notify_settings_get"),
    invokeBound<unknown>("whoop_status"),
    // The profile never changes within a launch: asked once, kept once it answered.
    cache.profile ? Promise.resolve(cache.profile) : invokeBound<unknown>("system_profile"),
    getServicesStatus(),
    getServicesSummary(),
    invokeBound<unknown>("model_tiers"),
    invokeBound<unknown>("setup_done"),
  ]);
  cache.secrets = s.status === "fulfilled" ? s.value : null;
  cache.keyFile = k.status === "fulfilled" ? k.value : null;
  cache.backupSettings = bs.status === "fulfilled" && isSettings(bs.value) ? bs.value : null;
  cache.backupList = bl.status === "fulfilled" && Array.isArray(bl.value) && bl.value.every(isRow) ? bl.value : null;
  cache.map = sm.status === "fulfilled" && sm.value && Array.isArray(sm.value.nodes) && Array.isArray(sm.value.paths) ? sm.value : null;
  cache.relay = rl.status === "fulfilled" ? toRelay(rl.value) : null;
  cache.whoop = wh.status === "fulfilled" && isWhoop(wh.value) ? wh.value : null;
  cache.profile = pr.status === "fulfilled" && isProfile(pr.value) ? pr.value : null;
  // An older backend has no supervisor commands: both stay null and the system-map derivation is used.
  cache.sup = sv.status === "fulfilled" && Array.isArray(sv.value) && sv.value.length > 0 && sv.value.every(isSupStatus) ? sv.value : null;
  cache.supSummary = ss.status === "fulfilled" ? summaryFromSupervisor(ss.value) : null;
  cache.tiers = td.status === "fulfilled" ? toTiers(td.value) : null;
  cache.setupDone = st.status === "fulfilled" && typeof st.value === "boolean" ? st.value : null;
  notify();
}

/** `backup://progress` `{ phase, pct }`: shown as the running bar while a backup runs. */
function listenBackupProgress() {
  try {
    void listen<{ phase: string; pct: number }>("backup://progress", (e) => {
      const { phase, pct } = e.payload ?? ({} as { phase?: string; pct?: number });
      if (typeof phase !== "string" || typeof pct !== "number" || !cache.run) return;
      // Bytes are not reported, so done/total/left stay empty rather than invented.
      cache.run = { pct: Math.max(0, Math.min(100, Math.round(pct))), phase, done: "", total: "", left: "" };
      notify();
    }).catch(() => {});
  } catch { /* no Tauri event bridge (tests, browser preview) */ }
}

/** `model://progress` `{ role, state, pct, bytesDone, bytesTotal }`. */
function listenModelProgress() {
  try {
    void listen<unknown>("model://progress", (e) => {
      const next = mergeProgress(cache.models, e.payload);
      if (next !== cache.models) { cache.models = next; notify(); }
    }).catch(() => {});
  } catch { /* no Tauri event bridge (tests, browser preview) */ }
}

let started = false;
export function makeLiveStandalone(base: StandaloneAdapter): StandaloneAdapter {
  if (!started) { started = true; void refreshLive().then(() => verifyLast()); listenModelProgress(); listenBackupProgress(); }
  return {
    ...base,
    // The supervisor is the source when it answers; otherwise the system-map derivation (older ids). Never a guessed state.
    services: () => live(cache.sup ? servicesFromSupervisor(cache.sup) : cache.map ? servicesFromMap(cache.map) : null),
    summary: () => live(cache.sup ? (cache.supSummary ?? summaryFromServices(servicesFromSupervisor(cache.sup).services)) : cache.map ? summaryFromServices(servicesFromMap(cache.map).services) : null),
    serviceLogs: (id: string) => askLogs(id),
    // Restart only (the supervisor's "start" has no backend). Every restart asks in the native approval dialog; a
    // decline, `unsupported`, `needs_you:` or failure comes back unwired and only a fixed code is logged.
    serviceAction: async (id: string, action: "restart" | "start") => {
      if (action !== "restart") return UNWIRED;
      try { await invokeBound("service_restart", { id }); } catch (e) { console.warn("[standalone] service_restart refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    tiers: () => live(cache.tiers),
    setupDone: () => live(cache.setupDone),
    setupDownload: () => toSetupDownload(cache.models, cache.sup, cache.startRows),
    // Local only: records nothing on disk; refuses an id the manifest did not offer.
    pickTier: async (tier: string) => (cache.tiers?.some((t) => t.id === tier && t.available) ? OK : UNWIRED),
    startDownload: async (tier: string) => {
      cache.models = {}; downloadedTier = null; cache.startRows = null;
      try { await invokeBound("model_download", { tier }); downloadedTier = tier; } catch (e) { console.warn("[standalone] model_download refused:", setupCode(e)); return UNWIRED; }
      return OK;
    },
    finishSetup: async (tier: string) => {
      if (!setupReady(downloadedTier, tier, cache.startRows)) { console.warn("[standalone] setup_complete not called:", "setup_not_ready"); return UNWIRED; }
      try { await invokeBound("setup_complete", { tier }); } catch (e) { console.warn("[standalone] setup_complete refused:", setupCode(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    startServices: async () => {
      try { cache.startRows = await invokeBound<StartAllRow[]>("services_start_all"); } catch (e) { console.warn("[standalone] services_start_all refused:", setupCode(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    resetSetup: async () => {
      try { await invokeBound("setup_reset"); } catch (e) { console.warn("[standalone] setup_reset refused:", setupCode(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    whoop: () => toWhoop(cache.whoop),
    profile: () => toProfile(cache.profile),
    restorePreview: () => askRestorePreview(),
    secrets: () => live(cache.secrets),
    setSecret: async (name: SecretName, value: string) => {
      try { await invokeBound("secret_set", { name, value }); } catch (e) { console.warn("[standalone] secret_set refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    backup: () => {
      const { backupSettings: settings, backupList: list } = cache;
      if (!settings || !list) return UNWIRED;
      // `last` is the newest backup_list row plus the backup_verify_last result. `version` is not
      // in the file name, so it stays empty instead of a made-up value.
      return live<BackupStatus>({
        last: mapLast(list, cache.verify),
        running: cache.run,
        settings,
        backups: list.map((r) => ({ id: r.id, label: r.label, madeAt: fmtMadeAt(r.madeAt), version: "", daysOld: r.daysOld })),
      });
    },
    backupNow: async () => {
      cache.run = { pct: 0, phase: "Starting", done: "", total: "", left: "" };
      notify();
      try { await invokeBound("backup_now"); } catch (e) {
        console.warn("[standalone] backup_now refused:", code(e));
        cache.run = null;
        await refreshLive();
        return UNWIRED;
      }
      cache.run = null;
      await refreshLive();
      return OK;
    },
    setBackupSettings: async (patch: Partial<BackupSettings>) => {
      const { nightly, at, dir, keep, beforeUpdate, skipOnBatteryBelow } = patch;
      const clean = Object.fromEntries(Object.entries({ nightly, at, dir, keep, beforeUpdate, skipOnBatteryBelow }).filter(([, v]) => v !== undefined));
      try { await invokeBound("backup_settings_set", { patch: clean }); } catch (e) { console.warn("[standalone] backup_settings_set refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    relay: () => live(cache.relay),
    setRelay: async (p: Partial<RelaySettings>) => {
      const patch = { enabled: p.enabled, kinds: p.kinds };
      try { cache.relay = toRelay(await invokeBound("notify_settings_set", { patch })) ?? cache.relay; } catch (e) { console.warn("[standalone] notify_settings_set refused:", code(e)); await refreshLive(); return UNWIRED; }
      notify();
      return OK;
    },
    relayPreview: () => { askPreview(); return live(cache.preview); },
    relaySendTest: async () => {
      try { await invokeBound("notify_send_test"); } catch (e) { console.warn("[standalone] notify_send_test refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    keyFile: () => live(cache.keyFile),
    makeKeyFile: async (passphrase: string) => {
      try { await invokeBound("key_file_make", { passphrase }); } catch (e) { console.warn("[standalone] key_file_make refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
  };
}
