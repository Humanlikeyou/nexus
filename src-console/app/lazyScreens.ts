import { lazy } from "react";

// Code-split screens. The default screen (TerranceField) stays eager in App so
// first paint has no empty frame; the rest load on first visit.
export const HealthField = lazy(() => import("../screens/HealthScreen").then((m) => ({ default: m.HealthField })));
export const NeuroField = lazy(() => import("../screens/NeuroScreen").then((m) => ({ default: m.NeuroField })));
export const SessionsField = lazy(() => import("../screens/SessionsScreen").then((m) => ({ default: m.SessionsField })));
export const DevicesField = lazy(() => import("../screens/DevicesScreen").then((m) => ({ default: m.DevicesField })));
export const SettingsField = lazy(() => import("../screens/SettingsScreen").then((m) => ({ default: m.SettingsField })));
export const BusinessField = lazy(() => import("../screens/BusinessScreen").then((m) => ({ default: m.BusinessField })));
export const RevenueField = lazy(() => import("../screens/RevenueScreen").then((m) => ({ default: m.RevenueField })));
export const ActivityField = lazy(() => import("../screens/ActivityScreen").then((m) => ({ default: m.ActivityField })));
export const TrajectoryField = lazy(() => import("../screens/TrajectoryScreen").then((m) => ({ default: m.TrajectoryField })));
