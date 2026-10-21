import type { ModuleId } from "../types";
import { Activity, BriefcaseBusiness, Bluetooth, Bot, CircleDollarSign, HeartPulse, History, Radio, Route, Settings2, type LucideIcon } from "lucide-react";

export const SYSTEM_GROUPS: Array<{ label: string; systems: Array<{ id: ModuleId; label: string; signal: string; icon: LucideIcon }> }> = [
  { label: "Nexus", systems: [
    { id: "terrance", label: "Overview", signal: "AI", icon: Bot },
    { id: "neuro", label: "Live", signal: "EEG", icon: Radio },
    { id: "sessions", label: "Sessions", signal: "HISTORY", icon: History },
    { id: "health", label: "Health", signal: "NOOP", icon: HeartPulse },
  ] },
  { label: "Direction", systems: [
    { id: "trajectory", label: "Trajectory", signal: "GOALS", icon: Route },
    { id: "business", label: "Business", signal: "WORK", icon: BriefcaseBusiness },
    { id: "revenue", label: "Revenue", signal: "ARM", icon: CircleDollarSign },
  ] },
  { label: "System", systems: [
    { id: "devices", label: "Devices", signal: "BLE", icon: Bluetooth },
    { id: "assistant", label: "Activity", signal: "LOG", icon: Activity },
    { id: "settings", label: "Settings", signal: "APP", icon: Settings2 },
  ] },
];

export const SYSTEMS = SYSTEM_GROUPS.flatMap((group) => group.systems);
