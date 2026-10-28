import { lazy, type ComponentType } from "react";

// Code-split screens. The default screen (TerranceField) stays eager in App so
// first paint has no empty frame; the rest load on first visit.
//
// React.lazy caches a rejected import forever, so each screen is wrapped in a
// stable component whose inner lazy() can be replaced by retryLazyScreens()
// (called from the screen error boundary's Retry).

const resetters: Array<() => void> = [];

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function retryableLazy<C extends ComponentType<any>>(load: () => Promise<{ default: C }>): C {
  let Inner = lazy(load);
  resetters.push(() => { Inner = lazy(load); });
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const Screen = (props: any) => <Inner {...props} />;
  return Screen as unknown as C;
}

/** Drop cached (possibly failed) lazy imports so the next render re-imports. */
export function retryLazyScreens(): void {
  for (const reset of resetters) reset();
}

export const HealthField = retryableLazy(() => import("../screens/HealthScreen").then((m) => ({ default: m.HealthField })));
export const NeuroField = retryableLazy(() => import("../screens/NeuroScreen").then((m) => ({ default: m.NeuroField })));
export const SessionsField = retryableLazy(() => import("../screens/SessionsScreen").then((m) => ({ default: m.SessionsField })));
export const DevicesField = retryableLazy(() => import("../screens/DevicesScreen").then((m) => ({ default: m.DevicesField })));
export const SettingsField = retryableLazy(() => import("../screens/SettingsScreen").then((m) => ({ default: m.SettingsField })));
export const BusinessField = retryableLazy(() => import("../screens/BusinessScreen").then((m) => ({ default: m.BusinessField })));
export const RevenueField = retryableLazy(() => import("../screens/RevenueScreen").then((m) => ({ default: m.RevenueField })));
export const CrewsField = retryableLazy(() => import("../screens/CrewsScreen").then((m) => ({ default: m.CrewsField })));
export const ActivityField = retryableLazy(() => import("../screens/ActivityScreen").then((m) => ({ default: m.ActivityField })));
export const TrajectoryField = retryableLazy(() => import("../screens/TrajectoryScreen").then((m) => ({ default: m.TrajectoryField })));
