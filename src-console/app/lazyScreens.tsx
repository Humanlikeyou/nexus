import { lazy, useState, type ComponentType } from "react";
import type { ModuleId } from "../types";

// Code-split screens. The default screen (TerranceField) stays eager in App so
// first paint has no empty frame; the rest load on first visit.
//
// React.lazy caches a rejected import forever, so each screen is wrapped in a
// stable component whose inner lazy() can be replaced by retryLazyScreens()
// (called from the screen error boundary's Retry).
//
// A chunk that has already loaded renders directly, not through lazy(), so opening it never
// suspends: Suspense would otherwise commit its blank skeleton into the route transition.

const resetters: Array<() => void> = [];
const preloaders = new Map<ModuleId, () => Promise<void> | null>();

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function retryableLazy<C extends ComponentType<any>>(screen: ModuleId, importScreen: () => Promise<{ default: C }>): C {
  let loaded: C | null = null;
  const load = () => importScreen().then((module) => { loaded = module.default; return module; });
  let Inner = lazy(load);
  resetters.push(() => { Inner = lazy(load); });
  preloaders.set(screen, () => (loaded ? null : load().then(() => undefined, () => undefined)));
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const Screen = (props: any) => {
    // Decided once per mount: swapping element types later would remount the screen and lose its state.
    const [Ready] = useState<C | null>(() => loaded);
    return Ready ? <Ready {...props} /> : <Inner {...props} />;
  };
  return Screen as unknown as C;
}

/** Load a screen's chunk. Null when there is nothing to wait for. Never rejects: a failed load
 *  surfaces through the screen's own error boundary. */
export function preloadScreen(screen: ModuleId): Promise<void> | null {
  return preloaders.get(screen)?.() ?? null;
}

/** Load every chunk one at a time after launch. Returns a cancel for the effect that started it. */
export function preloadAllScreens(): () => void {
  let cancelled = false;
  const timer = window.setTimeout(async () => {
    for (const screen of preloaders.keys()) {
      if (cancelled) return;
      await preloadScreen(screen);
    }
  }, 300);
  return () => { cancelled = true; window.clearTimeout(timer); };
}

/** Drop cached (possibly failed) lazy imports so the next render re-imports. */
export function retryLazyScreens(): void {
  for (const reset of resetters) reset();
}

export const HealthField = retryableLazy("health", () => import("../screens/HealthScreen").then((m) => ({ default: m.HealthField })));
export const NeuroField = retryableLazy("neuro", () => import("../screens/NeuroScreen").then((m) => ({ default: m.NeuroField })));
export const SessionsField = retryableLazy("sessions", () => import("../screens/SessionsScreen").then((m) => ({ default: m.SessionsField })));
export const DevicesField = retryableLazy("devices", () => import("../screens/DevicesScreen").then((m) => ({ default: m.DevicesField })));
export const SettingsField = retryableLazy("settings", () => import("../screens/SettingsScreen").then((m) => ({ default: m.SettingsField })));
export const BusinessField = retryableLazy("business", () => import("../screens/BusinessScreen").then((m) => ({ default: m.BusinessField })));
export const RevenueField = retryableLazy("revenue", () => import("../screens/RevenueScreen").then((m) => ({ default: m.RevenueField })));
export const CrewsField = retryableLazy("crews", () => import("../screens/CrewsScreen").then((m) => ({ default: m.CrewsField })));
export const ActivityField = retryableLazy("assistant", () => import("../screens/ActivityScreen").then((m) => ({ default: m.ActivityField })));
export const VoiceField = retryableLazy("voice", () => import("../screens/VoiceScreen").then((m) => ({ default: m.VoiceField })));
export const SmartHomeField = retryableLazy("smarthome", () => import("../screens/SmartHomeScreen").then((m) => ({ default: m.SmartHomeField })));
export const MindBodyField = retryableLazy("mindbody", () => import("../screens/MindBodyScreen").then((m) => ({ default: m.MindBodyField })));
export const LifeField = retryableLazy("life", () => import("../life/LifeScreen").then((m) => ({ default: m.LifeField })));
export const ListsField = retryableLazy("lists", () => import("../screens/ListsScreen").then((m) => ({ default: m.ListsField })));
