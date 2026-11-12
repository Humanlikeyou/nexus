import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { BackupRun, BackupSettings, BackupStatus, KeyFileStatus, SecretName, SecretStatus, StandaloneAdapter } from "./adapters";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { servicesFromMap } from "./servicesFromMap";
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
type Cache = {
  secrets: SecretStatus[] | null;
  keyFile: KeyFileStatus | null;
  backupSettings: BackupSettings | null;
  backupList: BackupRow[] | null;
  run: BackupRun | null;
  map: SystemMap | null;
};
const cache: Cache = { secrets: null, keyFile: null, backupSettings: null, backupList: null, run: null, map: null };
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

export async function refreshLive(): Promise<void> {
  const [s, k, bs, bl, sm] = await Promise.allSettled([
    invoke<SecretStatus[]>("secrets_status"),
    invoke<KeyFileStatus>("key_file_status"),
    invoke<BackupSettings>("backup_settings_get"),
    invoke<BackupRow[]>("backup_list"),
    invoke<SystemMap>("system_map_status"),
  ]);
  cache.secrets = s.status === "fulfilled" ? s.value : null;
  cache.keyFile = k.status === "fulfilled" ? k.value : null;
  cache.backupSettings = bs.status === "fulfilled" && isSettings(bs.value) ? bs.value : null;
  cache.backupList = bl.status === "fulfilled" && Array.isArray(bl.value) && bl.value.every(isRow) ? bl.value : null;
  cache.map = sm.status === "fulfilled" && sm.value && Array.isArray(sm.value.nodes) && Array.isArray(sm.value.paths) ? sm.value : null;
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
  if (!started) { started = true; void refreshLive(); listenBackupProgress(); }
  return {
    ...base,
    services: () => live(cache.map ? servicesFromMap(cache.map) : null),
    secrets: () => live(cache.secrets),
    setSecret: async (name: SecretName, value: string) => {
      try { await invoke("secret_set", { name, value }); } catch (e) { console.warn("[standalone] secret_set refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
    backup: () => {
      const { backupSettings: settings, backupList: list } = cache;
      if (!settings || !list) return UNWIRED;
      // `last` (needs a stored checksum result) and `version` (not in the file name) have no
      // source yet, so `last` is null and `version` is empty instead of a made-up value.
      return live<BackupStatus>({
        last: null,
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
    keyFile: () => live(cache.keyFile),
    makeKeyFile: async (passphrase: string) => {
      try { await invoke("key_file_make", { passphrase }); } catch (e) { console.warn("[standalone] key_file_make refused:", code(e)); return UNWIRED; }
      await refreshLive();
      return OK;
    },
  };
}
