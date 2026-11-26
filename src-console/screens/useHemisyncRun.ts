import { useCallback, useEffect, useRef, useState } from "react";
import { NEXUS_NOTICE_EVENT } from "../legacy-core/terrance/system/selfDiagnosis";
import type { HemisyncAdapter, HemisyncTrack } from "../neuro/hemisync/adapters";

/** A quiet in-app confirmation (the shell shows it as a toast). */
export function notice(message: string) {
  try { window.dispatchEvent(new CustomEvent(NEXUS_NOTICE_EVENT, { detail: { message } })); } catch { /* no window */ }
}

/**
 * The one Start flow (decision 0010): the track and the Muse recording start together and stop together.
 * The Muse itself is started and stopped by the existing Neuro session path (`startMuse` / `stopMuse`);
 * this hook only owns the audio, the link to the Muse session id, and the finish call.
 */
export type RunPhase = "idle" | "starting" | "playing";

/** Decision 0059: the next track in the same wave, in its order. Never played on its own; it is offered. */
export function nextInWave(tracks: HemisyncTrack[], current: HemisyncTrack | null): HemisyncTrack | null {
  if (!current || current.wave === 0) return null;
  const same = tracks.filter((t) => t.wave === current.wave && t.src).sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
  const at = same.findIndex((t) => t.id === current.id);
  return at >= 0 && at + 1 < same.length ? same[at + 1] : null;
}

const VOLUME_KEY = "nexus.hemisync.volume";
function savedVolume(): number {
  try { const v = Number(localStorage.getItem(VOLUME_KEY)); return Number.isFinite(v) && v >= 0 && v <= 1 && localStorage.getItem(VOLUME_KEY) !== null ? v : 1; } catch { return 1; }
}
export type Finished = { sessionId: string; trackId: string; title: string } | null;

type Run = { track: HemisyncTrack; startedAt: number; startedIso: string; sessionId: string | null; linked: boolean };

export type HemisyncRunInput = {
  adapter: HemisyncAdapter;
  /** Muse connected (idle or recording). */
  connected: boolean;
  /** A Muse recording is running right now. */
  recording: boolean;
  /** Muse session id of the running recording (null until it exists). */
  sessionId: string | null;
  operation: string | null;
  protocolId: string;
  setProtocolId: (id: string) => void;
  startMuse: () => void;
  stopMuse: () => void;
  /** Re-read the adapter after a write. */
  changed: () => void;
};

export function useHemisyncRun({ adapter, connected, recording, sessionId, operation, protocolId, setProtocolId, startMuse, stopMuse, changed }: HemisyncRunInput) {
  const [active, setActive] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [unsaved, setUnsaved] = useState(false);
  const [finished, setFinished] = useState<Finished>(null);
  const [position, setPosition] = useState({ at: 0, of: 0 });
  // Decision 0059: playback controls. Pausing the track never stops the Muse recording.
  const [paused, setPaused] = useState(false);
  const [volume, setVolumeState] = useState(savedVolume);
  const pendingStart = useRef(false);
  const audio = useRef<HTMLAudioElement | null>(null);
  const run = useRef<Run | null>(null);
  const sawStarting = useRef(false);
  const live = useRef({ adapter, stopMuse, changed, recording });
  useEffect(() => { live.current = { adapter, stopMuse, changed, recording }; });

  const listened = () => {
    const el = audio.current;
    return el ? Math.max(0, Math.round(el.currentTime * 1000)) : 0;
  };

  /** Stops the audio and tells the backend how long it was heard. `stopRecording` also stops the Muse. */
  const finish = useCallback((completed: boolean, stopRecording: boolean) => {
    const current = run.current;
    if (!current) return;
    run.current = null;
    const el = audio.current;
    const heard = completed && el && Number.isFinite(el.duration) ? Math.round(el.duration * 1000) : listened();
    if (el) { el.pause(); el.removeAttribute("src"); el.load?.(); }
    audio.current = null;
    setActive(false);
    setPaused(false);
    setPosition({ at: 0, of: 0 });
    if (stopRecording && live.current.recording) live.current.stopMuse();
    const id = current.sessionId;
    if (!id) { setMessage("The Muse didn't start, so the track was stopped."); return; }
    void live.current.adapter.finishSession({ sessionId: id, listenedMs: heard, completed }).then((result) => {
      if (result.state === "unwired") setUnsaved(true);
      live.current.changed();
      setFinished({ sessionId: id, trackId: current.track.id, title: current.track.title });
    }, () => { setUnsaved(true); setFinished({ sessionId: id, trackId: current.track.id, title: current.track.title }); });
  }, []);

  const begin = useCallback(async (track: HemisyncTrack) => {
    setMessage(null);
    setUnsaved(false);
    if (!connected) { setMessage("Connect your Muse first. Start records with the Muse and plays the track together, so it won't play on its own."); return; }
    if (!track.src) { setMessage("This track has no audio file here, so nothing was started."); return; }
    if (recording && protocolId !== "off") { setMessage("A training session is already running with feedback sounds. Stop it, then press Start so Hemi-Sync records only."); return; }
    const el = new Audio(track.src);
    el.preload = "auto";
    el.volume = volume;
    try {
      await el.play(); // from the click, so the browser allows sound
    } catch {
      setMessage("The track couldn't be played, so the Muse wasn't started either. Try importing the collection again.");
      return;
    }
    audio.current = el;
    sawStarting.current = false;
    run.current = { track, startedAt: Date.now(), startedIso: new Date().toISOString(), sessionId: recording ? sessionId : null, linked: false };
    el.addEventListener("ended", () => finish(true, true));
    el.addEventListener("timeupdate", () => setPosition({ at: el.currentTime, of: Number.isFinite(el.duration) ? el.duration : 0 }));
    setActive(true);
    if (recording) return; // a recording is already running: the track joins it
    if (protocolId !== "off") { setProtocolId("off"); pendingStart.current = true; } else startMuse();
  }, [connected, recording, sessionId, protocolId, setProtocolId, startMuse, finish, volume]);

  // Hemi-Sync records only (no feedback sounds on top of the track). Start once the protocol has switched.
  useEffect(() => {
    if (pendingStart.current && protocolId === "off") { pendingStart.current = false; startMuse(); }
  }, [protocolId, startMuse]);

  // The Muse recording has an id: link it to the track, once. If the Muse never starts, or stops by itself, the track stops too.
  useEffect(() => {
    const current = run.current;
    if (!active || !current) return;
    if (operation === "starting") sawStarting.current = true;
    if (recording && sessionId) {
      current.sessionId = sessionId;
      if (!current.linked) {
        current.linked = true;
        void adapter.linkSession({ sessionId, trackId: current.track.id, startedAt: current.startedIso }).then((result) => { if (result.state === "unwired") setUnsaved(true); changed(); }, () => setUnsaved(true));
      }
    } else if (current.linked && !recording) {
      finish(false, false); // the recording ended some other way (Stop session, disconnect)
    } else if (!current.linked && !recording && operation === null && sawStarting.current && !pendingStart.current) {
      current.sessionId = null; // the Muse didn't start: no audio on its own
      finish(false, false);
    }
  }, [active, recording, sessionId, operation, adapter, changed, finish]);

  // Leaving the screen mid-track stops the audio and says how much was heard. The Muse recording carries on.
  useEffect(() => () => {
    const current = run.current;
    if (!current) return;
    audio.current?.pause();
    if (current.sessionId) void live.current.adapter.finishSession({ sessionId: current.sessionId, listenedMs: listened(), completed: false });
  }, []);

  const stop = useCallback(() => finish(false, true), [finish]);
  const pause = useCallback(() => { const el = audio.current; if (el && !el.paused) { el.pause(); setPaused(true); } }, []);
  const resume = useCallback(() => { const el = audio.current; if (el && el.paused) void el.play().then(() => setPaused(false), () => setMessage("The track couldn't resume. Stop and start it again.")); }, []);
  /** Moves the track by `deltaS` seconds, kept inside the track. */
  const seek = useCallback((deltaS: number) => {
    const el = audio.current;
    if (!el || !Number.isFinite(deltaS)) return;
    const end = Number.isFinite(el.duration) ? el.duration : Infinity;
    el.currentTime = Math.min(Math.max(0, el.currentTime + deltaS), Math.max(0, end - 0.5));
    setPosition({ at: el.currentTime, of: Number.isFinite(el.duration) ? el.duration : 0 });
  }, []);
  const setVolume = useCallback((v: number) => {
    const clamped = Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 1;
    setVolumeState(clamped);
    if (audio.current) audio.current.volume = clamped;
    try { localStorage.setItem(VOLUME_KEY, String(clamped)); } catch { /* only a convenience */ }
  }, []);
  const rate = useCallback(async (rating: 1 | 2 | 3 | 4 | 5, note: string) => {
    if (!finished) return;
    const result = await adapter.rate({ sessionId: finished.sessionId, rating, ...(note.trim() ? { note: note.trim() } : {}) }).catch(() => null);
    const ok = !!result && result.state !== "unwired";
    if (!ok) setUnsaved(true);
    notice(ok ? `Rated ${finished.title} ${rating} of 5` : "The rating couldn't be saved yet");
    changed();
    setFinished(null);
  }, [adapter, changed, finished]);

  const phase: RunPhase = !active ? "idle" : recording && sessionId ? "playing" : "starting";
  return { phase, message, unsaved, finished, dismissFinished: () => setFinished(null), position, begin, stop, rate, paused, pause, resume, seek, volume, setVolume };
}
