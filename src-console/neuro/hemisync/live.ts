import { invokeBound } from "../../bridge";
import { listen } from "@tauri-apps/api/event";
import { UNWIRED, sampleAdaptersEnabled, type Wired } from "../../lib/sampleAdapters";
import { announceStores, inTauri, live } from "../../lib/localStores";
import {
  makeSampleHemisync, unwiredHemisync,
  type HemisyncAdapter, type HemisyncLibrary, type HemisyncMetrics, type HemisyncSession, type HemisyncTrack, type HemisyncTrackStats, type HemisyncWave, type ImportProgress,
} from "./adapters";

/**
 * Hemi-Sync live adapter (decision 0010) over the Rust commands in src-tauri/src/commands/hemisync.rs.
 * Wired semantics: every read is synchronous over a cache, refreshed after each call and when the
 * backend reports import progress. A store is wired only after its first successful read, so a
 * missing backend keeps showing "Needs wiring". Nothing here invents a value.
 */

// Shapes the Rust commands return (camelCase).
type RawTrack = { id: string; wave: number; waveName: string; order: number; title: string; durationMs: number | null };
type RawLibrary = { tracks: RawTrack[]; importedAt: string | null; sourceFolder: string | null };
type RawSession = { id: string; trackId: string; startedAt: string; listenedMs: number; completed: boolean; rating: number | null; note: string | null; metrics: { focusAvg: number | null; calmAvg: number | null; bands: HemisyncMetrics["bands"] | null } };

export type HemisyncDeps = {
  call: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  /** Track id -> playable URL on the `hemisync` scheme. Never a file path. */
  toSrc: (id: string) => string;
  listenImport: (cb: (p: ImportProgress) => void) => void;
  notify: () => void;
};

const defaultDeps: HemisyncDeps = {
  call: (command, args) => invokeBound(command, args),
  toSrc: (id) => hemisyncSrc(id),
  listenImport: (cb) => { void listen<ImportProgress>("hemisync://import", (e) => cb(e.payload)).catch(() => undefined); },
  notify: announceStores,
};

/**
 * URL the audio element plays: the Rust `hemisync` URI scheme serves it by track id only
 * (`hemisync://localhost/<id>` on macOS/Linux, `http://hemisync.localhost/<id>` on Windows).
 * An id that is not 16 lowercase hex characters gives "" (unplayable).
 */
export function hemisyncSrc(id: string, windows: boolean = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent)): string {
  if (!/^[0-9a-f]{16}$/.test(id)) return "";
  return windows ? `http://hemisync.localhost/${id}` : `hemisync://localhost/${id}`;
}

const finiteOrNull = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const asRating = (v: number | null): HemisyncSession["rating"] => (v === 1 || v === 2 || v === 3 || v === 4 || v === 5 ? v : null);

export function mapLibrary(raw: RawLibrary, srcs: Record<string, string>): HemisyncLibrary {
  const byWave = new Map<number, HemisyncWave>();
  for (const t of raw.tracks) {
    const track: HemisyncTrack = { id: t.id, wave: t.wave, waveName: t.waveName, order: t.order, title: t.title, durationMs: finiteOrNull(t.durationMs), src: srcs[t.id] ?? "" };
    const w = byWave.get(t.wave) ?? { wave: t.wave, waveName: t.waveName, tracks: [] };
    w.tracks.push(track);
    byWave.set(t.wave, w);
  }
  const waves = [...byWave.values()].sort((a, b) => a.wave - b.wave);
  for (const w of waves) w.tracks.sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
  return { waves, importedAt: raw.importedAt, sourceFolder: raw.sourceFolder };
}

export function mapSession(r: RawSession): HemisyncSession {
  return {
    id: r.id, trackId: r.trackId, startedAt: r.startedAt, listenedMs: r.listenedMs, completed: r.completed,
    rating: asRating(r.rating), note: r.note,
    metrics: { focusAvg: finiteOrNull(r.metrics?.focusAvg), calmAvg: finiteOrNull(r.metrics?.calmAvg), bands: r.metrics?.bands ?? null },
  };
}

export function makeLiveHemisync(deps: HemisyncDeps = defaultDeps): HemisyncAdapter {
  let lib: HemisyncLibrary | null = null;
  let sessions: HemisyncSession[] | null = null;
  let stats: HemisyncTrackStats[] | null = null;
  let progress: ImportProgress | null = null;
  let started = false;
  let inflight: Promise<void> | null = null;

  async function readLibrary(): Promise<void> {
    const raw = await deps.call<RawLibrary>("hemisync_library");
    const srcs: Record<string, string> = {};
    for (const t of raw.tracks) srcs[t.id] = deps.toSrc(t.id);
    lib = mapLibrary(raw, srcs);
  }
  async function readSessions(): Promise<void> {
    sessions = (await deps.call<RawSession[]>("hemisync_sessions")).map(mapSession);
  }
  async function readStats(): Promise<void> {
    stats = (await deps.call<HemisyncTrackStats[]>("hemisync_stats")).map((s) => ({
      trackId: s.trackId, sessions: s.sessions, lastAt: s.lastAt,
      avgRating: finiteOrNull(s.avgRating), focusAvg: finiteOrNull(s.focusAvg), calmAvg: finiteOrNull(s.calmAvg),
    }));
  }
  /** One store failing leaves the others (and any earlier cache) as they were. */
  function refresh(): Promise<void> {
    inflight ??= Promise.allSettled([readLibrary(), readSessions(), readStats()])
      .then(() => undefined)
      .finally(() => { inflight = null; deps.notify(); });
    return inflight;
  }
  function start() {
    if (started) return;
    started = true;
    deps.listenImport((p) => { progress = p; deps.notify(); });
    void refresh();
  }
  async function write<T>(command: string, args: Record<string, unknown>, result: T): Promise<Wired<T>> {
    try {
      await deps.call(command, args);
    } catch {
      return UNWIRED;
    }
    await refresh();
    return live(result);
  }
  const read = <T,>(v: T | null): Wired<T> => { start(); return v === null ? UNWIRED : live(v); };

  return {
    library: () => read(lib),
    sessions: () => read(sessions),
    stats: () => read(stats),
    importProgress: () => progress,
    async importFrom(folder) {
      start();
      progress = { copied: 0, total: 0, phase: "starting" };
      deps.notify();
      try {
        const r = await deps.call<{ added: number; skipped: number }>("hemisync_import", { folder: folder ?? null });
        progress = null;
        await refresh();
        return live({ added: r.added, skipped: r.skipped });
      } catch {
        progress = null;
        deps.notify();
        return UNWIRED;
      }
    },
    linkSession: (i) => write("hemisync_session_link", { sessionId: i.sessionId, trackId: i.trackId, startedAt: i.startedAt }, { ok: true as const }),
    finishSession: (i) => write("hemisync_session_finish", { sessionId: i.sessionId, listenedMs: Math.max(0, Math.round(i.listenedMs)), completed: i.completed }, { ok: true as const }),
    rate: (i) => write("hemisync_session_rate", { sessionId: i.sessionId, rating: i.rating, note: i.note ?? null }, { ok: true as const }),
  };
}

/**
 * The adapter the Neuro screens use (decision 0010). Sample in mock mode; the live adapter inside
 * Tauri; otherwise unwired ("Needs wiring").
 */
let sampleSingleton: HemisyncAdapter | null = null;
let liveSingleton: HemisyncAdapter | null = null;
export function hemisyncAdapter(): HemisyncAdapter {
  if (sampleAdaptersEnabled()) return (sampleSingleton ??= makeSampleHemisync());
  if (inTauri()) return (liveSingleton ??= makeLiveHemisync());
  return unwiredHemisync;
}
