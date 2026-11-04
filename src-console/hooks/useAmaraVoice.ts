import { useEffect, useSyncExternalStore } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Amara Voice → UI bridge (spec §7). Rust owns the emit; the UI only listens.
 * One shared pair of listeners (ref-counted across hook users), removed when the last user unmounts.
 * Levels go to a ref (read in rAF by AmaraLens), never React state; state changes are the only re-renders.
 */
export const AMARA_VOICE_LEVEL_EVENT = "amara-voice://level";
export const AMARA_VOICE_STATE_EVENT = "amara-voice://state";
export const AMARA_VOICE_STATES = ["idle", "listening", "thinking", "speaking", "error"] as const;
export type AmaraVoiceState = (typeof AMARA_VOICE_STATES)[number];
export const AMARA_VOICE_REASONS = ["unavailable", "tts_failed", "model_loading"] as const;
export type AmaraVoiceReason = (typeof AMARA_VOICE_REASONS)[number];

/** Assumed payloads (confirm with Rust). */
export interface AmaraVoiceLevel { ts: number; rms: number; peak: number; state: AmaraVoiceState; utteranceId?: string }
export interface AmaraVoiceStateEvent { ts: number; state: AmaraVoiceState; utteranceId?: string; reason?: AmaraVoiceReason }

export const WATCHDOG_MS = 2000;
export const STALE_MS = 500;
const WATCHDOG_TICK_MS = 250;

export interface AmaraVoiceSnapshot { state: AmaraVoiceState; reason: AmaraVoiceReason | null }
export interface LevelSample { rms: number; peak: number }

const clamp01 = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0);
const isState = (v: unknown): v is AmaraVoiceState => typeof v === "string" && (AMARA_VOICE_STATES as readonly string[]).includes(v);
const isReason = (v: unknown): v is AmaraVoiceReason => typeof v === "string" && (AMARA_VOICE_REASONS as readonly string[]).includes(v);

const IDLE: AmaraVoiceSnapshot = { state: "idle", reason: null };
let snapshot: AmaraVoiceSnapshot = IDLE;
/** Latest level target; AmaraLens smooths it inside requestAnimationFrame. */
export const amaraVoiceLevel: { current: LevelSample } = { current: { rms: 0, peak: 0 } };
let lastEventAt = 0;
let utterance: { id: string; ts: number } | null = null;
const subscribers = new Set<() => void>();
let users = 0;
let unlisteners: UnlistenFn[] = [];
let attachToken = 0;
let watchdog: ReturnType<typeof setInterval> | null = null;

function setSnapshot(next: AmaraVoiceSnapshot) {
  if (next.state === snapshot.state && next.reason === snapshot.reason) return;
  snapshot = next;
  if (next.state === "idle") amaraVoiceLevel.current = { rms: 0, peak: 0 };
  if (next.state === "idle") stopWatchdog(); else startWatchdog();
  subscribers.forEach((s) => s());
}

function startWatchdog() {
  if (watchdog) return;
  watchdog = setInterval(() => {
    // No event of either kind for 2s while not idle: level decays (release curve in the lens) and we go idle.
    if (snapshot.state !== "idle" && Date.now() - lastEventAt > WATCHDOG_MS) setSnapshot(IDLE);
  }, WATCHDOG_TICK_MS);
}
function stopWatchdog() { if (watchdog) { clearInterval(watchdog); watchdog = null; } }

/** Drops stale (>500ms) events and events from an older utterance once a newer one started. */
function accept(payload: { ts?: unknown; utteranceId?: unknown }): boolean {
  const now = Date.now();
  const ts = typeof payload.ts === "number" && Number.isFinite(payload.ts) ? payload.ts : now;
  if (now - ts > STALE_MS) return false;
  const id = typeof payload.utteranceId === "string" ? payload.utteranceId : null;
  if (id) {
    if (utterance && id !== utterance.id && ts < utterance.ts) return false;
    if (!utterance || id !== utterance.id) utterance = { id, ts };
  }
  lastEventAt = now;
  return true;
}

export function ingestLevel(raw: unknown) {
  if (!raw || typeof raw !== "object") return;
  const p = raw as Partial<Record<keyof AmaraVoiceLevel, unknown>>;
  if (!accept(p)) return;
  amaraVoiceLevel.current = { rms: clamp01(p.rms), peak: clamp01(p.peak) };
  if (isState(p.state) && p.state !== snapshot.state) setSnapshot({ state: p.state, reason: p.state === "error" ? snapshot.reason : null });
}

export function ingestState(raw: unknown) {
  if (!raw || typeof raw !== "object") return;
  const p = raw as Partial<Record<keyof AmaraVoiceStateEvent, unknown>>;
  if (!isState(p.state) || !accept(p)) return;
  if (p.state === "idle" || p.state === "thinking") amaraVoiceLevel.current = { rms: 0, peak: 0 };
  setSnapshot({ state: p.state, reason: isReason(p.reason) ? p.reason : null });
}

function attach() {
  const token = ++attachToken;
  const add = (p: Promise<UnlistenFn>) => p.then((un) => {
    if (token !== attachToken) un(); else unlisteners.push(un);
  }).catch(() => { /* Amara Voice bridge unavailable: stay idle (not error) until a state event says so */ });
  add(listen<unknown>(AMARA_VOICE_LEVEL_EVENT, (e) => ingestLevel(e.payload)));
  add(listen<unknown>(AMARA_VOICE_STATE_EVENT, (e) => ingestState(e.payload)));
}
function detach() {
  attachToken++;
  unlisteners.forEach((un) => un());
  unlisteners = [];
  stopWatchdog();
}

const subscribe = (cb: () => void) => { subscribers.add(cb); return () => { subscribers.delete(cb); }; };
const getSnapshot = () => snapshot;

/** `{state, reason, levelRef}`; re-renders on state transitions only. */
export function useAmaraVoice() {
  useEffect(() => {
    if (users++ === 0) attach();
    return () => { if (--users === 0) detach(); };
  }, []);
  const snap = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  return { state: snap.state, reason: snap.reason, levelRef: amaraVoiceLevel };
}

export function __resetAmaraVoiceForTests() {
  detach();
  users = 0;
  snapshot = IDLE;
  amaraVoiceLevel.current = { rms: 0, peak: 0 };
  lastEventAt = 0;
  utterance = null;
  subscribers.clear();
}
