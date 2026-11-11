import { useSyncExternalStore } from "react";
import { sampleAdaptersEnabled } from "../lib/sampleAdapters";
import { navyAdapter } from "./adapters";
import { nextOffLine, nextOnLine, resetLines } from "./lines";
import { parseNavyPrompt } from "./parse";
import { initialNavyState, NAVY_PREVIEW_KEY, type NavyPreview, type NavySource, type NavyState } from "./types";

let state: NavyState = initialNavyState();
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export const HOLD_MS = 1500;
export const ENTER_MS = 1400;
export const EXIT_MS = 800;
export const REDUCE_MS = 200;

export const navyStore = {
  get(): NavyState { return state; },
  subscribe(listener: () => void): () => void {
    listeners.add(listener);
    return () => { listeners.delete(listener); };
  },
  /** Tests and a simulated restart. The live flag is never written to disk. */
  reset(): void {
    state = initialNavyState();
    resetLines();
    emit();
  },
};

function set(patch: Partial<NavyState>): void {
  state = { ...state, ...patch };
  emit();
}

export function reducedNavyMotion(): boolean {
  if (typeof document === "undefined") return false;
  return document.documentElement.dataset.reduceMotion === "true"
    || (typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
}

function captionFor(kind: "on" | "off", openedBy: NavySource): NavyState["caption"] {
  const text = kind === "on" ? nextOnLine() : nextOffLine();
  const spoken = openedBy === "voice";
  const spokenResult = spoken ? navyAdapter().speak(text) : null;
  const sample = sampleAdaptersEnabled() || spokenResult?.state === "sample";
  return { text, sample, spoken };
}

/** Turn the look on. No-op if it's already entering or on. */
export function enterNavy(openedBy: NavySource): void {
  if (state.phase === "on" || state.phase === "entering") return;
  set({ phase: "entering", openedBy, caption: captionFor("on", openedBy), hold: "idle", holdAt: 0 });
}

/** Turn it off. `hold` is the orb fallback; voice/chat use the same write path as enter. */
export function exitNavy(openedBy: NavySource): void {
  if (state.phase === "off" || state.phase === "exiting") return;
  set({ phase: "exiting", openedBy, caption: captionFor("off", openedBy), hold: "idle", holdAt: 0 });
}

export function finishEnter(): void {
  if (state.phase !== "entering") return;
  set({ phase: "on" });
}

export function finishExit(): void {
  if (state.phase !== "exiting") return;
  set({ phase: "off", openedBy: null, caption: null, hold: "idle", holdAt: 0 });
}

export function beginHold(): void {
  if (state.phase !== "on" || state.hold === "confirm") return;
  set({ hold: "holding", holdAt: 0 });
}

export function setHoldAt(holdAt: number): void {
  if (state.hold !== "holding") return;
  set({ holdAt: Math.min(1, Math.max(0, holdAt)) });
}

export function cancelHold(): void {
  if (state.hold === "idle") return;
  set({ hold: "idle", holdAt: 0 });
}

export function reachHoldConfirm(): void {
  if (state.hold !== "holding") return;
  set({ hold: "confirm", holdAt: 1 });
}

export function confirmHold(): void {
  if (state.hold !== "confirm") return;
  exitNavy("hold");
}

/** Single write path for voice and typed chat. Returns true if the phrase was handled. */
export function handleNavyPrompt(text: string, openedBy: Exclude<NavySource, "hold" | "preview">): boolean {
  const intent = parseNavyPrompt(text);
  if (!intent) return false;
  if (intent === "on") enterNavy(openedBy);
  else exitNavy(openedBy);
  return true;
}

export function readNavyPreview(): NavyPreview | null {
  try {
    const raw = globalThis.localStorage?.getItem(NAVY_PREVIEW_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as NavyPreview;
  } catch { return null; }
}

/** Test / capture hook. Does not persist the live flag — only reads a preview key. */
export function applyNavyPreview(p: NavyPreview): void {
  if (p.scenario === "off") {
    state = initialNavyState();
    emit();
    return;
  }
  const openedBy = p.openedBy ?? "preview";
  if (p.scenario === "on" || p.scenario === "holding" || p.scenario === "confirm") {
    const hold = p.scenario === "confirm" ? "confirm" as const : p.scenario === "holding" ? "holding" as const : "idle" as const;
    state = { phase: "on", openedBy, caption: { text: "Lights out.", sample: true, spoken: openedBy === "voice" }, hold, holdAt: hold === "holding" ? 0.6 : hold === "confirm" ? 1 : 0 };
    emit();
    return;
  }
  // Mid-glitch frames: entering, with a frozen beat applied by the root from the scenario name.
  state = { phase: "entering", openedBy, caption: { text: "The room just got smaller.", sample: true, spoken: openedBy === "voice" }, hold: "idle", holdAt: 0 };
  emit();
}

export function useNavyState(): NavyState {
  return useSyncExternalStore(navyStore.subscribe, navyStore.get, navyStore.get);
}
