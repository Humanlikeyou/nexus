/**
 * Router "model loading" (HTTP 503 + Retry-After) → automatic retries.
 * Single swap point for how Rust reports it. Accepted today:
 *  - an object `{ kind: "model_loading", retryAfter?: seconds }` (proposed)
 *  - a string `"model_loading: retry_after=<seconds>"` (Rust, confirmed: 1–300, default 10)
 *  - Rust's current fixed text for 429/503 ("Hermes is busy with another request…"), which has
 *    no Retry-After, so the default wait applies.
 */
export const MODEL_LOADING_DEFAULT_MS = 3000;
/** Rust sends retry_after 1–300 s (default 10). 60 s per wait (3 waits ≤ 3 min, Cancel always works) covers a deep model load without leaving the bubble stuck for 5 min. */
export const MODEL_LOADING_MAX_MS = 60_000;
export const MODEL_LOADING_MIN_MS = 500;
export const MODEL_LOADING_RETRIES = 3;
export const MODEL_LOADING_COPY = "Amara is loading her model…";
export const MODEL_LOADING_FAILED = "Amara's model is still loading. Try again in a moment.";
const HERMES_BUSY = /^Hermes is busy with another request/i;

const clampMs = (seconds: number | null): number => {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return MODEL_LOADING_DEFAULT_MS;
  return Math.min(MODEL_LOADING_MAX_MS, Math.max(MODEL_LOADING_MIN_MS, Math.round(seconds * 1000)));
};

/** Retry-After as seconds (integer or decimal); HTTP-date forms are ignored (default applies). */
export function parseRetryAfter(value: unknown): number | null {
  if (typeof value === "number") return value;
  if (typeof value !== "string") return null;
  const v = value.trim();
  return /^\d+(\.\d+)?$/.test(v) ? Number(v) : null;
}

/** Wait before the next try, or null when the error isn't "model loading". */
export function modelLoadingDelay(err: unknown): number | null {
  if (err && typeof err === "object" && !(err instanceof Error)) {
    const o = err as Record<string, unknown>;
    if (o.kind === "model_loading") return clampMs(parseRetryAfter(o.retryAfter ?? o.retry_after));
    return null;
  }
  const text = err instanceof Error ? err.message : typeof err === "string" ? err : "";
  if (/^\s*model_loading\b/i.test(text)) return clampMs(parseRetryAfter(/retry_after=([\d.]+)/i.exec(text)?.[1]));
  if (HERMES_BUSY.test(text)) return MODEL_LOADING_DEFAULT_MS;
  return null;
}

/** setTimeout that resolves early (with false) when aborted. */
export function waitUnlessAborted(ms: number, signal: AbortSignal): Promise<boolean> {
  return new Promise((resolve) => {
    if (signal.aborted) { resolve(false); return; }
    const id = setTimeout(() => { signal.removeEventListener("abort", onAbort); resolve(true); }, ms);
    const onAbort = () => { clearTimeout(id); resolve(false); };
    signal.addEventListener("abort", onAbort, { once: true });
  });
}
