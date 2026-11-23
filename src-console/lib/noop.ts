import { invokeBound } from "../bridge";
import type { BodyState, NoopDayRow, NoopMetricKey, NoopMetricPoint, NoopMetricSeries, NoopQueryArgs, NoopQueryError, NoopQueryTool, NoopSeriesPayload, NoopSnapshotPayload } from "../types";

export type NoopRead<T> = { ok: true; value: T } | { ok: false; error: NoopQueryError };

function asError(e: unknown): NoopQueryError {
  if (e && typeof e === "object" && "kind" in e && typeof (e as { kind: unknown }).kind === "string") return e as NoopQueryError;
  // Command not registered yet (Rust side not built) or other invoke failure.
  return { kind: "spawn_failed", detail: "noop_query unavailable" };
}

/**
 * Typed wrapper over the Tauri command `noop_query(tool, args)` (Integrations' contract, 10s timeout).
 * Never logs results or args: biometric values stay out of the console. Only the error kind is logged.
 */
export async function noopQuery(tool: NoopQueryTool, args: NoopQueryArgs | null = null): Promise<NoopRead<unknown>> {
  try {
    return { ok: true, value: await invokeBound<unknown>("noop_query", { tool, args }) };
  } catch (e) {
    const error = asError(e);
    console.warn(`[console] noop_query ${tool} failed: ${error.kind}`);
    return { ok: false, error };
  }
}

/**
 * Several reads in ONE backend session (`noop_query_batch`: one spawn instead of one per read).
 * Results come back in call order, each as a NoopRead, so callers treat them like single reads.
 * A session-level failure (binary missing, pin, timeout...) fails every slot with that error. When
 * the batch command itself is unavailable (older backend, mocks) it falls back to parallel single
 * reads, so behaviour is unchanged. Never logs results or args.
 */
export async function noopQueryBatch(calls: Array<{ tool: NoopQueryTool; args?: NoopQueryArgs | null }>): Promise<NoopRead<unknown>[]> {
  const payload = calls.map((c) => ({ tool: c.tool, args: c.args ?? null }));
  try {
    const items = await invokeBound<Array<{ value?: unknown; error?: NoopQueryError | null }>>("noop_query_batch", { calls: payload });
    if (!Array.isArray(items) || items.length !== calls.length) throw new Error("batch shape");
    return items.map((it, i): NoopRead<unknown> => {
      if (it && it.error) { console.warn(`[console] noop_query ${calls[i].tool} failed: ${it.error.kind}`); return { ok: false, error: it.error }; }
      return { ok: true, value: it?.value ?? null };
    });
  } catch (e) {
    if (e && typeof e === "object" && "kind" in e && typeof (e as { kind: unknown }).kind === "string") {
      const error = e as NoopQueryError;
      console.warn(`[console] noop_query batch failed: ${error.kind}`);
      return calls.map(() => ({ ok: false, error }));
    }
    return Promise.all(calls.map((c) => noopQuery(c.tool, c.args ?? null)));
  }
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const nn = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** Thrown when a payload doesn't match the agreed shape; the UI treats it like bad_output (retry state). */
export class NoopShapeError extends Error { constructor(what: string) { super(`noop ${what}: unexpected shape`); } }

/** metric_series → points (one per day, ascending, value never null). `points: []` is an empty state, not an error. */
export function parseMetricSeries(key: NoopMetricKey, raw: unknown): NoopMetricSeries {
  if (!isObj(raw) || !Array.isArray(raw.points)) throw new NoopShapeError("metric_series");
  const points: NoopMetricPoint[] = [];
  for (const p of raw.points as NoopSeriesPayload["points"]) {
    if (isObj(p) && typeof p.day === "string") points.push({ day: p.day, value: nn(p.value) });
  }
  return { key, points };
}

export function isSnapshot(raw: unknown): raw is NoopSnapshotPayload {
  // NOOP omits nil fields, so today, latestSleep, latestNight and battery can each be absent.
  return isObj(raw) && typeof raw.generatedAt === "string" && Array.isArray(raw.recentDays);
}

/** recentDays plus today (deduped by day, ascending). */
export function snapshotDays(snap: NoopSnapshotPayload): NoopDayRow[] {
  const map = new Map(snap.recentDays.map((d) => [d.day, d]));
  if (snap.today) map.set(snap.today.day, snap.today);
  return [...map.values()].sort((a, b) => a.day.localeCompare(b.day));
}

/**
 * Session body_context (minimised snake_case: { today, latest_night }) → body state.
 * Sleep prefers latest_night.total_sleep_min, else today.sleep_total_min. Null when absent or all-null.
 */
export function parseBodyContext(raw: unknown): BodyState | null {
  if (!isObj(raw) || !("today" in raw) || !("latest_night" in raw)) return null;
  const today = isObj(raw.today) ? raw.today : null;
  const night = isObj(raw.latest_night) ? raw.latest_night : null;
  const ctx: BodyState = {
    captured_at: typeof night?.day === "string" ? night.day : typeof today?.day === "string" ? today.day : null,
    recovery: nn(today?.recovery),
    hrv_ms: nn(today?.hrv),
    resting_hr: nn(today?.rhr),
    sleep_minutes: nn(night?.total_sleep_min) ?? nn(today?.sleep_total_min),
  };
  return ctx.recovery === null && ctx.hrv_ms === null && ctx.resting_hr === null && ctx.sleep_minutes === null ? null : ctx;
}
