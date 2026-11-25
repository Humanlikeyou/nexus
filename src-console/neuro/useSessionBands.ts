import { useEffect, useState } from "react";
import { getSessionSamples, type SessionSamplesIpc } from "../bridge";
import { inTauri } from "../lib/localStores";
import { hasSamples, type BandKey, type SessionSamples, type SessionWithSamples } from "./sessionSamples";

/**
 * Decision 0045: the Neuro rings show brain data, so the newest few sessions need their band series.
 * `list_sessions` only attaches samples when a current `.bands.json` sidecar already exists and never builds
 * one; `get_session_samples` builds it. This reads the recording read-only (the only write is the allowed
 * derived `.bands.json` cache beside it), one session at a time, newest first, and remembers what it got.
 */
export const BAND_FETCH_LIMIT = 5;
const BAND_KEYS: BandKey[] = ["delta", "theta", "alpha", "beta", "gamma"];

const finite = (xs: ReadonlyArray<number | null> | undefined): number[] => (xs ?? []).filter((v): v is number => typeof v === "number" && Number.isFinite(v));

/** The IPC series (gaps are null) as the screen's samples: gaps dropped; `null` when there is nothing to draw. */
export function samplesFromIpc(r: SessionSamplesIpc | null): SessionSamples | null {
  if (!r || !(r.stepSecs > 0)) return null;
  const bands = Object.fromEntries(BAND_KEYS.map((k) => [k, finite(r.bands?.[k])])) as Record<BandKey, number[]>;
  if (!BAND_KEYS.some((k) => bands[k].length >= 2)) return null;
  return { stepSecs: r.stepSecs, bands, focus: finite(r.focus), calm: finite(r.calm) };
}

const got = new Map<string, SessionSamples>();

/** `sessions` with band samples merged in for the newest few that lack them. Gaps and failures leave a session as it was. */
export function useSessionBands<T extends SessionWithSamples>(sessions: readonly T[]): T[] {
  const [, setTick] = useState(0);
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set());
  const next = inTauri() ? sessions.slice(0, BAND_FETCH_LIMIT).find((s) => !hasSamples(s) && !got.has(s.id) && !skipped.has(s.id)) : undefined;
  const nextId = next?.id;
  useEffect(() => {
    if (!nextId) return;
    let live = true;
    void getSessionSamples(nextId).then(
      (r) => {
        const s = samplesFromIpc(r);
        if (s) got.set(nextId, s);
        if (live) { if (s) setTick((n) => n + 1); else setSkipped((p) => new Set(p).add(nextId)); }
      },
      () => { if (live) setSkipped((p) => new Set(p).add(nextId)); },
    );
    return () => { live = false; };
  }, [nextId]);
  return sessions.map((s) => (!hasSamples(s) && got.has(s.id) ? { ...s, samples: got.get(s.id) } : s));
}
