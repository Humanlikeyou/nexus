import { useSyncExternalStore } from "react";

/**
 * design-1118b Stage B: every step behind its own Settings toggle. Persisted in localStorage.
 * - lens (a): the sand lens; "classic" keeps the old static lens selectable.
 * - tokens (b): light, depth and motion tokens (lit edges, shadows from the light, parallax).
 * - ambience (c): bio-reactive ambience from real WHOOP recovery (neutral fallback, never invented).
 * - moments (e): micro-moments. - sound (f): OFF by default.
 */
export type SandMaterial = "sand" | "fireflies";
export type SandSettings = {
  lens: "sand" | "classic";
  tokens: boolean; ambience: boolean; moments: boolean; sound: boolean;
  material: SandMaterial; calm: boolean; volume: number; mute: boolean;
};
export const SAND_DEFAULTS: SandSettings = { lens: "sand", tokens: true, ambience: true, moments: true, sound: false, material: "sand", calm: false, volume: 0.6, mute: false };
export const SAND_KEY = "nexus.sandLens";
export const SAND_EVENT = "nexus:sand-settings";

let cached: SandSettings | null = null;
function read(): SandSettings {
  try {
    const raw = window.localStorage.getItem(SAND_KEY);
    const v = raw ? (JSON.parse(raw) as Partial<SandSettings>) : {};
    const m = v.material === "fireflies" ? "fireflies" : "sand"; // "spores" (Stage A) is dropped → sand
    return { ...SAND_DEFAULTS, ...v, material: m, lens: v.lens === "classic" ? "classic" : "sand", volume: typeof v.volume === "number" ? Math.min(1, Math.max(0, v.volume)) : SAND_DEFAULTS.volume };
  } catch { return { ...SAND_DEFAULTS }; }
}
export function sandSettings(): SandSettings { return (cached ??= read()); }
export function setSandSettings(patch: Partial<SandSettings>) {
  cached = { ...sandSettings(), ...patch };
  try { window.localStorage.setItem(SAND_KEY, JSON.stringify(cached)); } catch { /* storage unavailable */ }
  window.dispatchEvent(new Event(SAND_EVENT));
}
export function __resetSandSettings() { cached = null; }
function subscribe(cb: () => void) {
  const on = () => { cached = null; cb(); };
  window.addEventListener(SAND_EVENT, cb);
  window.addEventListener("storage", on);
  return () => { window.removeEventListener(SAND_EVENT, cb); window.removeEventListener("storage", on); };
}
export function useSandSettings(): SandSettings { return useSyncExternalStore(subscribe, sandSettings, () => SAND_DEFAULTS); }

/** Calm switch OR the OS / app reduced-motion preference. */
export function calmActive(s: SandSettings): boolean {
  if (s.calm) return true;
  if (typeof document !== "undefined" && document.documentElement.getAttribute("data-reduce-motion") === "true") return true;
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Deterministic per-session seed for the organic edge (no flicker across renders, new each session). */
export function sessionSeed(): number {
  try {
    const k = "nexus.sandSeed";
    const v = window.sessionStorage.getItem(k);
    if (v) return Number(v);
    const s = Math.floor(Math.random() * 100000) / 100000 * 97 + 1;
    window.sessionStorage.setItem(k, String(s));
    return s;
  } catch { return 13.37; }
}
