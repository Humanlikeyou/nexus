import { invokeBound } from "../bridge";
import { classifyHaError } from "./homeAssistant";

/**
 * Home Assistant connection settings. Single swap point for the Rust commands
 * (ha_get_config / ha_set_config / ha_clear_token / ha_test_connection).
 * The token is write-only: it is sent once on save and never read back, logged or echoed.
 */
export type HaTokenSource = "app" | "env" | "legacy";
export type HaConfig = { url: string | null; tokenSet: boolean; tokenSource: HaTokenSource | null };
export type HaClearResult = { fallbackActive: boolean };
export type HaTestKind = "unreachable" | "auth_rejected" | "upstream_error" | "too_large" | "bad_url" | "not_configured" | "not_allowed" | "already_running" | "unsafe_storage" | "token_required";
export type HaTestResult = { ok: boolean; kind?: string; version?: string };
export type HaConfigInput = { url?: string; token?: string };

export interface HaConfigBackend {
  getConfig(): Promise<HaConfig>;
  setConfig(input: HaConfigInput): Promise<void>;
  clearToken(): Promise<HaClearResult>;
  /** Tests the given (unsaved, edited) values; omitted keys fall back to the saved config. */
  testConnection(input?: HaConfigInput): Promise<HaTestResult>;
}

const tauriBackend: HaConfigBackend = {
  async getConfig() {
    const raw = await invokeBound<Partial<HaConfig> | null>("ha_get_config");
    // Pick only the two known fields so nothing else (e.g. a token) can reach the UI.
    const src = raw?.tokenSource;
    return {
      url: typeof raw?.url === "string" && raw.url !== "" ? raw.url : null,
      tokenSet: raw?.tokenSet === true,
      tokenSource: src === "app" || src === "env" || src === "legacy" ? src : null,
    };
  },
  setConfig: (input) => {
    // Flat args matching ha_set_config(url: Option<String>, token: Option<String>); omitted keys stay unchanged.
    const args: Record<string, string> = {};
    if (input.url !== undefined) args.url = input.url;
    if (input.token !== undefined) args.token = input.token;
    return invokeBound<void>("ha_set_config", args);
  },
  async clearToken() {
    const raw = await invokeBound<Partial<HaClearResult> | null>("ha_clear_token");
    return { fallbackActive: raw?.fallbackActive === true };
  },
  async testConnection(input = {}) {
    // Flat args: ha_test_connection(url?: String, token?: String). Blank counts as absent, so it isn't sent.
    const args: Record<string, string> = {};
    if (input.url) args.url = input.url;
    if (input.token) args.token = input.token;
    const raw = await invokeBound<HaTestResult | null>("ha_test_connection", args);
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
const LOOPBACK = /^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(?::(\d{1,5}))?\/?$/i;

/** Client-side check: loopback http with an optional port; no path, query or credentials. Empty clears (default address). */
export function urlError(value: string): string | null {
  const v = value.trim();
  if (v === "") return null;
  if (/^https:\/\//i.test(v)) return "Use http://, not https://, for Home Assistant on this Mac.";
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

export const SET_ERROR_KINDS = ["bad_url", "invalid_input", "unsafe_storage", "storage_error", "not_allowed", "upstream_error"] as const;
export type HaSetErrorKind = (typeof SET_ERROR_KINDS)[number];
export const SET_ERROR_COPY: Record<HaSetErrorKind, string> = {
  bad_url: "Nexus didn't accept that address. Use a local address like http://127.0.0.1:8123.",
  invalid_input: "Nexus didn't accept those details. Check the address and token, then try again.",
  unsafe_storage: "Nexus couldn't store the token safely. Its settings file has unsafe permissions or ownership.",
  storage_error: "Nexus couldn't save the settings. Try again.",
  not_allowed: "Nexus doesn't allow this change right now.",
  upstream_error: "Something went wrong while saving. Try again.",
};

/** Fixed copy for save/clear failures, by "<kind>: …" prefix; the backend's message is never shown. */
export function setErrorCopy(err: unknown, fallback = "Couldn't save the Home Assistant settings. Try again."): string {
  const text = err instanceof Error ? err.message : typeof err === "string" ? err : "";
  const kind = /^\s*([a-z_]+):/.exec(text)?.[1];
  return kind && (SET_ERROR_COPY as Record<string, string>)[kind] ? (SET_ERROR_COPY as Record<string, string>)[kind] : fallback;
}
/** @deprecated use setErrorCopy */
export const saveErrorCopy = (err: unknown) => setErrorCopy(err);

export const TOKEN_SOURCE_COPY: Record<HaTokenSource, string> = {
  app: "Saved in Nexus",
  env: "From environment",
  legacy: "From legacy config",
};
export const FALLBACK_ACTIVE_COPY = "A token from the environment is still in use.";

/** Security rule: testing an edited address needs the token re-entered (Rust returns kind token_required). */
export const TOKEN_REQUIRED_COPY = "Re-enter the token to test a new address.";

/** Comparable form of an HA address, matching the backend: trimmed, trailing slash dropped. No host aliasing (localhost ≠ 127.0.0.1), no case folding. */
export function normalizeHaUrl(value: string | null | undefined): string {
  return (value ?? "").trim().replace(/\/+$/, "");
}

export const TEST_COPY: Record<HaTestKind, string> = {
  unreachable: "Home Assistant isn't reachable. Check that it's running.",
  auth_rejected: "Home Assistant rejected the token. Create a new long-lived token and replace it.",
  upstream_error: "Home Assistant returned an error. Try again later.",
  too_large: "Home Assistant sent a response that was too large.",
  bad_url: "The saved address isn't a local http address. Fix it, then test again.",
  not_configured: "Add the address and token, then test again.",
  not_allowed: "Nexus doesn't allow a connection test right now.",
  already_running: "A test is already running. Wait a moment, then try again.",
  token_required: TOKEN_REQUIRED_COPY,
  unsafe_storage: "Nexus couldn't read the token safely. Its settings file has unsafe permissions or ownership.",
};

export function testResultCopy(result: HaTestResult): string {
  if (result.ok) return result.version ? `Connected to Home Assistant ${result.version}.` : "Connected to Home Assistant.";
  return (result.kind && (TEST_COPY as Record<string, string>)[result.kind]) || "Couldn't test the connection. Try again.";
}

export const COMING_SOON_COPY = "Coming soon: needs Nexus update.";
