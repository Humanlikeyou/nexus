/**
 * useIdleAutoSave: wires summarizeAndStoreSessionSilent into the console chat.
 *
 * Saves a session summary when the transcript has been idle for `idleMs`, when
 * the window is hidden, when the conversation changes (new chat), and on
 * unmount. Each unique transcript (conversation + length + last message) is
 * saved at most once (signature includes a content hash, so edits re-save), persisted in localStorage so a relaunch that reloads
 * history does not re-summarize it.
 *
 * Fail-closed note: summarizeAndStoreSession aborts its context write if the
 * context cannot be loaded (bug 1); the session log is still stored.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { summarizeAndStoreSessionSilent, type SessionEntry } from "@/terrance/system/continuity";

export const IDLE_AUTOSAVE_MS = 10 * 60_000;
export const AUTOSAVE_SIGNATURE_KEY = "nexus.terrence.autosave.lastSavedSignature";

export interface AutoSaveMessage {
  role: "user" | "assistant";
  content: string;
  timestamp: string;
  status?: "failed";
}

interface Snapshot {
  key: string;
  entries: SessionEntry[];
}

export function toSessionEntries(messages: AutoSaveMessage[]): SessionEntry[] {
  return messages
    .filter((m) => m.status !== "failed" && m.content.trim().length > 0)
    .map((m) => ({
      role: m.role === "user" ? "user" : "terrance",
      text: m.content,
      ts: Number.isFinite(Date.parse(m.timestamp)) ? Date.parse(m.timestamp) : 0,
    }));
}

/** Pre-#11 signature (no content hash). Still recognized so upgrades don't re-summarize. */
function legacySignature(s: Snapshot): string {
  const last = s.entries[s.entries.length - 1];
  return `${s.key}:${s.entries.length}:${last?.ts ?? 0}`;
}

/** 32-bit FNV-1a over all message roles and text: cheap, catches edits anywhere. */
export function contentHash(entries: SessionEntry[]): string {
  let h = 0x811c9dc5;
  for (const e of entries) {
    const str = `${e.role}\u0000${e.text}\u0001`;
    for (let i = 0; i < str.length; i++) {
      h ^= str.charCodeAt(i);
      h = Math.imul(h, 0x01000193);
    }
  }
  return (h >>> 0).toString(36);
}

function signature(s: Snapshot): string {
  return `${legacySignature(s)}:h${contentHash(s.entries)}`;
}

function readSaved(): string | null {
  try { return window.localStorage.getItem(AUTOSAVE_SIGNATURE_KEY); } catch { return null; }
}
function writeSaved(sig: string): void {
  try { window.localStorage.setItem(AUTOSAVE_SIGNATURE_KEY, sig); } catch { /* storage unavailable */ }
}

export interface IdleAutoSave {
  /** Epoch ms of the last successful save this session (null until one lands). */
  lastSavedAt: number | null;
  /** Save the current transcript now (same rules: 2+ entries, deduped). */
  saveNow: () => void;
}

export function useIdleAutoSave(
  messages: AutoSaveMessage[],
  conversationKey: string,
  idleMs: number = IDLE_AUTOSAVE_MS,
): IdleAutoSave {
  const snapshot = useRef<Snapshot>({ key: conversationKey, entries: [] });
  const inFlight = useRef<string | null>(null);
  const mounted = useRef(true);
  const [lastSavedAt, setLastSavedAt] = useState<number | null>(null);

  const save = (s: Snapshot) => {
    if (s.entries.length < 2) return;
    const sig = signature(s);
    const saved = readSaved();
    if (sig === inFlight.current || sig === saved) return;
    if (saved === legacySignature(s)) {
      // Saved before content hashing existed: assume unchanged, upgrade in place.
      writeSaved(sig);
      return;
    }
    inFlight.current = sig;
    void summarizeAndStoreSessionSilent(s.entries)
      .then((r) => {
        if (!r.saved) return;
        writeSaved(sig);
        if (mounted.current) setLastSavedAt(Date.now());
      })
      .catch(() => { /* Silent variant should not throw; never let auto-save break the UI */ })
      .finally(() => { if (inFlight.current === sig) inFlight.current = null; });
  };

  // Keep the latest committed transcript (updated post-commit so cleanups see the previous one).
  useEffect(() => {
    snapshot.current = { key: conversationKey, entries: toSessionEntries(messages) };
  }, [messages, conversationKey]);

  // Idle timer: restart on every transcript change.
  useEffect(() => {
    if (messages.length < 2) return;
    const t = setTimeout(() => save(snapshot.current), idleMs);
    return () => clearTimeout(t);
  }, [messages, conversationKey, idleMs]);

  // Conversation switch (new chat): save the outgoing transcript.
  useEffect(() => {
    return () => save(snapshot.current);
  }, [conversationKey]);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  // Window hidden / closing.
  useEffect(() => {
    const onHide = () => { if (document.visibilityState === "hidden") save(snapshot.current); };
    const onPageHide = () => save(snapshot.current);
    document.addEventListener("visibilitychange", onHide);
    window.addEventListener("pagehide", onPageHide);
    return () => {
      document.removeEventListener("visibilitychange", onHide);
      window.removeEventListener("pagehide", onPageHide);
    };
  }, []);

  const saveNow = useCallback(() => save(snapshot.current), []);
  return { lastSavedAt, saveNow };
}
