import { invoke } from "@tauri-apps/api/core";
import { classifyHaError } from "./homeAssistant";

/**
 * Home Assistant connection settings. Single swap point for the Rust commands
 * (ha_get_config / ha_set_config / ha_clear_token / ha_test_connection).
 * The token is write-only: it is sent once on save and never read back, logged or echoed.
 */
export type HaConfig = { url: string | null; tokenSet: boolean };
export type HaTestKind = "not_configured" | "auth_rejected" | "unreachable" | "timed_out" | "upstream_error";
export type HaTestResult = { ok: boolean; kind?: string; version?: string };
export type HaConfigInput = { url?: string; token?: string };

export interface HaConfigBackend {
  getConfig(): Promise<HaConfig>;
  setConfig(input: HaConfigInput): Promise<void>;
  clearToken(): Promise<void>;
  testConnection(): Promise<HaTestResult>;
}

const tauriBackend: HaConfigBackend = {
  async getConfig() {
    const raw = await invoke<Partial<HaConfig> | null>("ha_get_config");
    // Pick only the two known fields so nothing else (e.g. a token) can reach the UI.
    return { url: typeof raw?.url === "string" ? raw.url : null, tokenSet: raw?.tokenSet === true };
  },
  setConfig: (input) => {
    // Flat args matching ha_set_config(url: Option<String>, token: Option<String>); omitted keys stay unchanged.
    const args: Record<string, string> = {};
    if (input.url !== undefined) args.url = input.url;
    if (input.token !== undefined) args.token = input.token;
    return invoke<void>("ha_set_config", args);
  },
  clearToken: () => invoke<void>("ha_clear_token"),
  async testConnection() {
    const raw = await invoke<HaTestResult | null>("ha_test_connection");
    return {
      ok: raw?.ok === true,
      kind: typeof raw?.kind === "string" ? raw.kind : undefined,
      version: typeof raw?.version === "string" ? raw.version : undefined,
    };
  },
};

let backend: HaConfigBackend = tauriBackend;
export const haConfigBackend = (): HaConfigBackend => backend;
/** Tests only. Returns a restore function. */
export function setHaConfigBackend(next: HaConfigBackend): () => void { const prev = backend; backend = next; return () => { backend = prev; }; }

/** True when the command isn't in this build yet (Tauri "command … not found"). */
export const isCommandMissing = (err: unknown): boolean => classifyHaError(err) === "backend_missing";

export const HA_DEFAULT_URL = "http://127.0.0.1:8123";
const LOOPBACK = /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(?::(\d{1,5}))?\/?$/i;

/** Client-side check: loopback http(s) with an optional port; no path, query or credentials. */
export function urlError(value: string): string | null {
  const v = value.trim();
  if (v === "") return "Enter the Home Assistant address.";
  const m = LOOPBACK.exec(v);
  if (!m) return "Use a local address on this Mac, like http://127.0.0.1:8123.";
  if (m[2] !== undefined) {
    const port = Number(m[2]);
    if (port < 1 || port > 65535) return "Use a port between 1 and 65535.";
  }
  return null;
}

export function tokenError(value: string): string | null {
  if (value === "") return null;
  if (/\s/.test(value)) return "Paste the token without spaces or line breaks.";
  if (value.length > 4096) return "That token is too long.";
  return null;
}

/** Fixed copy for save failures; the backend's message is never shown. */
export function saveErrorCopy(err: unknown): string {
  const text = err instanceof Error ? err.message : typeof err === "string" ? err : "";
  if (/^\s*bad_url\b/.test(text)) return "Nexus didn't accept that address. Use a local address like http://127.0.0.1:8123.";
  if (/^\s*invalid_input\b/.test(text)) return "Nexus didn't accept those details. Check the address and token, then try again.";
  return "Couldn't save the Home Assistant settings. Try again.";
}

export const TEST_COPY: Record<HaTestKind, string> = {
  not_configured: "Add the address and token, then test again.",
  auth_rejected: "Home Assistant rejected the token. Create a new long-lived token and replace it.",
  unreachable: "Home Assistant isn't reachable. Check that it's running.",
  timed_out: "Home Assistant took too long to answer. Try again.",
  upstream_error: "Home Assistant returned an error. Try again later.",
};

export function testResultCopy(result: HaTestResult): string {
  if (result.ok) return result.version ? `Connected to Home Assistant ${result.version}.` : "Connected to Home Assistant.";
  return (result.kind && (TEST_COPY as Record<string, string>)[result.kind]) || "Couldn't test the connection. Try again.";
}

export const COMING_SOON_COPY = "Coming soon: needs Nexus update.";
