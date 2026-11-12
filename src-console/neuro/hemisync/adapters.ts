import { UNWIRED, sample, type Wired } from "../../lib/sampleAdapters";

/**
 * Hemi-Sync in Neuro (decision 0010). The CONTRACT between the backend (Rust commands +
 * `live.ts`) and Design (the Neuro screens). Design renders only through `HemisyncAdapter`;
 * the backend implements it in `live.ts`. Keep `Wired<T>` semantics: unwired shows
 * "Needs wiring", sample only in mock mode (tagged "◇ Sample data"), never an invented number.
 */

export type HemisyncTrack = {
  id: string;            // stable id from the backend (relative file path hash)
  wave: number;          // 1–7 (Wave 1 Discovery … Wave 7 Odyssey); 0 if unknown
  waveName: string;      // "Discovery", "Threshold", … from the folder name
  order: number;         // track order within the wave (CD/track number), 0 if unknown
  title: string;         // human title parsed from the file name ("Intro to Focus 15")
  durationMs: number | null; // null until known (the player reads it from the audio metadata)
  src: string;           // playable URL (asset protocol), only ever under <app data>/hemisync/
};

export type HemisyncWave = { wave: number; waveName: string; tracks: HemisyncTrack[] };

export type HemisyncLibrary = {
  waves: HemisyncWave[];
  importedAt: string | null;       // ISO, last import
  sourceFolder: string | null;     // display only, e.g. "~/Desktop/Hemisync Collection"
};

/** Muse metrics for the recording linked to a session (from the existing session summary). null = not available. */
export type HemisyncMetrics = { focusAvg: number | null; calmAvg: number | null; bands: Partial<Record<"delta" | "theta" | "alpha" | "beta" | "gamma", number>> | null };

export type HemisyncSession = {
  id: string;            // the Muse session id it links to
  trackId: string;
  startedAt: string;     // ISO
  listenedMs: number;
  completed: boolean;    // reached the end of the track
  rating: 1 | 2 | 3 | 4 | 5 | null;
  note: string | null;
  metrics: HemisyncMetrics;
};

/** Per-track history for the "done with the Muse" view. */
export type HemisyncTrackStats = { trackId: string; sessions: number; lastAt: string | null; avgRating: number | null; focusAvg: number | null; calmAvg: number | null };

export type ImportProgress = { copied: number; total: number; phase: string };

export interface HemisyncAdapter {
  library(): Wired<HemisyncLibrary>;
  sessions(): Wired<HemisyncSession[]>;
  stats(): Wired<HemisyncTrackStats[]>;
  /** Copy audio from a folder (default ~/Desktop/Hemisync Collection) into the app. The source is only read. */
  importFrom(folder?: string): Promise<Wired<{ added: number; skipped: number }>>;
  importProgress(): ImportProgress | null;
  /** Called by the one Start flow once the Muse recording has a session id and the audio started. */
  linkSession(input: { sessionId: string; trackId: string; startedAt: string }): Promise<Wired<{ ok: true }>>;
  /** Called when the track ends or on Stop. */
  finishSession(input: { sessionId: string; listenedMs: number; completed: boolean }): Promise<Wired<{ ok: true }>>;
  /** The post-session sheet: 1–5 and an optional note. */
  rate(input: { sessionId: string; rating: 1 | 2 | 3 | 4 | 5; note?: string }): Promise<Wired<{ ok: true }>>;
}

export const unwiredHemisync: HemisyncAdapter = {
  library: () => UNWIRED, sessions: () => UNWIRED, stats: () => UNWIRED,
  importFrom: async () => UNWIRED, importProgress: () => null,
  linkSession: async () => UNWIRED, finishSession: async () => UNWIRED, rate: async () => UNWIRED,
};

/** Mock mode only. Titles are the real collection's names; every number is sample. */
export function makeSampleHemisync(): HemisyncAdapter {
  const t = (wave: number, waveName: string, order: number, title: string, min: number): HemisyncTrack =>
    ({ id: `s-${wave}-${order}`, wave, waveName, order, title, durationMs: min * 60_000, src: "" });
  const waves: HemisyncWave[] = [
    { wave: 1, waveName: "Discovery", tracks: [t(1, "Discovery", 1, "Orientation", 38), t(1, "Discovery", 2, "Intro to Focus 3", 34)] },
    { wave: 5, waveName: "Exploring", tracks: [t(5, "Exploring", 1, "Advanced Focus 12", 36), t(5, "Exploring", 4, "Intro to Focus 15", 40)] },
    { wave: 7, waveName: "Odyssey", tracks: [t(7, "Odyssey", 3, "Intro to Focus 25", 42)] },
  ];
  const sessions: HemisyncSession[] = [
    { id: "ses-1", trackId: "s-5-1", startedAt: "2026-11-10T21:30:00+09:00", listenedMs: 36 * 60_000, completed: true, rating: 4, note: "Settled fast", metrics: { focusAvg: 62, calmAvg: 71, bands: { alpha: 0.34, theta: 0.22 } } },
    { id: "ses-2", trackId: "s-5-4", startedAt: "2026-11-11T22:05:00+09:00", listenedMs: 28 * 60_000, completed: false, rating: 3, note: null, metrics: { focusAvg: 55, calmAvg: 66, bands: { alpha: 0.31, theta: 0.25 } } },
  ];
  const stats: HemisyncTrackStats[] = [
    { trackId: "s-5-1", sessions: 1, lastAt: sessions[0].startedAt, avgRating: 4, focusAvg: 62, calmAvg: 71 },
    { trackId: "s-5-4", sessions: 1, lastAt: sessions[1].startedAt, avgRating: 3, focusAvg: 55, calmAvg: 66 },
  ];
  const ok = sample({ ok: true as const });
  return {
    library: () => sample({ waves, importedAt: "2026-11-12T10:00:00+09:00", sourceFolder: "~/Desktop/Hemisync Collection" }),
    sessions: () => sample(sessions), stats: () => sample(stats),
    importFrom: async () => sample({ added: 0, skipped: 42 }), importProgress: () => null,
    linkSession: async () => ok, finishSession: async () => ok, rate: async () => ok,
  };
}
