import { invokeBound } from "../bridge";
import { useEffect, useState } from "react";
import type { Wired } from "./sampleAdapters";

/**
 * design-1111v live stores (decisions log, life dates, adds, check-in, Needs you today): the Rust commands in
 * src-tauri/src/commands/{decisions_log,life_dates,adds,checkin,needs_you}.rs. Adapters stay synchronous over a
 * cache; a store counts as wired only after its first successful read, so a missing backend (browser, e2e mock,
 * older build) keeps showing "Needs wiring" and never a made-up list.
 */
export type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export const realInvoke: Invoke = (command, args) => invokeBound(command, args);

/** True inside the Tauri webview. Tests and plain browsers have no IPC, so every adapter stays unwired there. */
export const inTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const live = <T,>(value: T): Wired<T> => ({ state: "live", value });

/** Fired when a live store finishes a first read or a write, so screens re-read their adapter. */
export const STORES_EVENT = "amara:stores";
export const announceStores = () => { try { window.dispatchEvent(new Event(STORES_EVENT)); } catch { /* no window in node tests */ } };
export function useStoresTick(): number {
  const [n, setN] = useState(0);
  useEffect(() => {
    const on = () => setN((x) => x + 1);
    window.addEventListener(STORES_EVENT, on);
    return () => window.removeEventListener(STORES_EVENT, on);
  }, []);
  return n;
}

/** Fired when something changed the system underneath the snapshot (a service restarted), so App's snapshot reloads now instead of at its next 15 s tick. */
export const SNAPSHOT_EVENT = "amara:snapshot";
export const announceSnapshot = () => { try { window.dispatchEvent(new Event(SNAPSHOT_EVENT)); } catch { /* no window in node tests */ } };

/** Fired after an add is saved or undone, so stores that list what adds write (life dates) re-read. */
export const ADDS_EVENT = "amara:adds";
export const announceAdds = () => { try { window.dispatchEvent(new Event(ADDS_EVENT)); } catch { /* no window in node tests */ } };

/** Needs you today, as src-tauri needs_you.rs returns it: one report per source, rows are pointers only. */
export type NeedsYouReport = { source: "todos" | "dates" | "checkin" | "decisions" | "crew" | "strap" | "watchlist"; state: "ok" | "failed" | "unwired"; rows: Array<{ source: string; id: string; title: string; due: string | null; reason: string; deepLink: { id: string; view: string }; badge?: { tone: "warning" | "accent" | "neutral"; label: string }; quiet?: boolean }> };
export const needsYouList = (call: Invoke = realInvoke) => call<NeedsYouReport[]>("needs_you_list");
