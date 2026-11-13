import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AlertKind, BackupRun, BackupSettings, BackupStatus, KeyFileStatus, RelayPreview, RelaySettings, SecretName, SecretStatus, StandaloneAdapter } from "./adapters";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { servicesFromMap } from "./servicesFromMap";
import { summaryFromServices } from "./servicesSummary";
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
};
const cache: Cache = { secrets: null, keyFile: null, backupSettings: null, backupList: null, verify: null, run: null, map: null, relay: null, preview: null };
let version = 0;
const listeners = new Set<() => void>();
const notify = () => { version += 1; listeners.forEach((l) => l()); };

export const subscribeLive = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };
export const liveVersion = () => version;

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
export function verifyLast(call: typeof invoke = invoke): Promise<void> {
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
  void invoke("notify_preview", { kind: "recovery" }).then((p) => { cache.preview = toPreview(p); }, () => { cache.preview = null; }).finally(notify);
}
export const resetRelayForTests = () => { previewAsked = false; cache.preview = null; cache.relay = null; };

export async function refreshLive(): Promise<void> {
  const [s, k, bs, bl, sm, rl] = await Promise.allSettled([
    invoke<SecretStatus[]>("secrets_status"),
    invoke<KeyFileStatus>("key_file_status"),
    invoke<BackupSettings>("backup_settings_get"),
    invoke<BackupRow[]>("backup_list"),
    invoke<SystemMap>("system_map_status"),
    invoke<unknown>("notify_settings_get"),
  ]);
  cache.secrets = s.status === "fulfilled" ? s.value : null;
  cache.keyFile = k.status === "fulfilled" ? k.value : null;
  cache.backupSettings = bs.status === "fulfilled" && isSettings(bs.value) ? bs.value : null;
  cache.backupList = bl.status === "fulfilled" && Array.isArray(bl.value) && bl.value.every(isRow) ? bl.value : null;
  cache.map = sm.status === "fulfilled" && sm.value && Array.isArray(sm.value.nodes) && Array.isArray(sm.value.paths) ? sm.value : null;
  cache.relay = rl.status === "fulfilled" ? toRelay(rl.value) : null;
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

let started = false;
export function makeLiveStandalone(base: StandaloneAdapter): StandaloneAdapter {
  if (!started) { started = true; void refreshLive().then(() => verifyLast()); listenBackupProgress(); }
  return {
    ...base,
    services: () => live(cache.map ? servicesFromMap(cache.map) : null),
    summary: () => live(cache.map ? summaryFromServices(servicesFromMap(cache.map).services) : null),
    secrets: () => live(cache.secrets),
    setSecret: async (name: SecretName, value: string) => {
      try { await invoke("secret_set", { name, value }); } catch (e) { console.warn("[standalone] secret_set refused:", code(e)); return UNWIRED; }
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
      try { await invoke("backup_now"); } catch (e) {
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
      try { await invoke("backup_settings_set", { patch: clean }); } catch (e) { console.warn("[standalone] backup_settings_set refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    relay: () => live(cache.relay),
    setRelay: async (p: Partial<RelaySettings>) => {
      const patch = { enabled: p.enabled, kinds: p.kinds };
      try { cache.relay = toRelay(await invoke("notify_settings_set", { patch })) ?? cache.relay; } catch (e) { console.warn("[standalone] notify_settings_set refused:", code(e)); await refreshLive(); return UNWIRED; }
      notify();
      return OK;
    },
    relayPreview: () => { askPreview(); return live(cache.preview); },
    relaySendTest: async () => {
      try { await invoke("notify_send_test"); } catch (e) { console.warn("[standalone] notify_send_test refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    keyFile: () => live(cache.keyFile),
    makeKeyFile: async (passphrase: string) => {
      try { await invoke("key_file_make", { passphrase }); } catch (e) { console.warn("[standalone] key_file_make refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
  };
}
