import { invoke } from "@tauri-apps/api/core";
import type { KeyFileStatus, SecretName, SecretStatus, StandaloneAdapter } from "./adapters";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";

/**
 * Live standalone adapter (HANDOFF-1111s wiring steps 1-2): secrets and the n8n key file, over
 * Tauri commands. Adapters are synchronous over already-loaded data, so this keeps a small cache that
 * `refresh()` fills; views re-render through `subscribeLive`. Everything not wired yet spreads the
 * unwired base. No value ever crosses back: the commands return `set` flags and status only, and
 * errors are logged by code, never with the value or passphrase.
 */
type Cache = { secrets: SecretStatus[] | null; keyFile: KeyFileStatus | null };
const cache: Cache = { secrets: null, keyFile: null };
let version = 0;
const listeners = new Set<() => void>();
const notify = () => { version += 1; listeners.forEach((l) => l()); };

export const subscribeLive = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };
export const liveVersion = () => version;

const live = <T,>(v: T | null): Wired<T> => (v === null ? UNWIRED : { state: "live", value: v });
const OK: Wired<{ ok: true }> = { state: "live", value: { ok: true } };
const code = (e: unknown) => (typeof e === "string" ? e.slice(0, 40) : "failed");

export async function refreshLive(): Promise<void> {
  const [s, k] = await Promise.allSettled([invoke<SecretStatus[]>("secrets_status"), invoke<KeyFileStatus>("key_file_status")]);
  cache.secrets = s.status === "fulfilled" ? s.value : null;
  cache.keyFile = k.status === "fulfilled" ? k.value : null;
  notify();
}

let started = false;
export function makeLiveStandalone(base: StandaloneAdapter): StandaloneAdapter {
  if (!started) { started = true; void refreshLive(); }
  return {
    ...base,
    secrets: () => live(cache.secrets),
    setSecret: async (name: SecretName, value: string) => {
      try { await invoke("secret_set", { name, value }); } catch (e) { console.warn("[standalone] secret_set refused:", code(e)); return UNWIRED; }
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
