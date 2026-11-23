import type { HaEntity } from "../lib/homeAssistant";
import { entityName } from "../lib/homeAssistant";

/**
 * design-1118a Smart home: household devices grouped by type, HA's own backup / container sensors in
 * System (never mixed with household devices), and a display name with doubled words removed
 * ("Backup Backup manager" → "Backup manager").
 */
export type HomeGroupId = "scenes" | "lights" | "switches" | "climate" | "media" | "covers" | "sensors" | "other" | "system";
export const GROUP_TITLE: Record<HomeGroupId, string> = {
  scenes: "Scenes", lights: "Lights", switches: "Switches", climate: "Climate", media: "Media", covers: "Covers and locks", sensors: "Sensors", other: "Other", system: "System",
};
const ORDER: HomeGroupId[] = ["scenes", "lights", "switches", "climate", "media", "covers", "sensors", "other", "system"];

const SYSTEM_RE = /\b(backup|hassio|supervisor|home assistant (core|os|host)|container)\b/i;
export const isSystemEntity = (e: HaEntity) => SYSTEM_RE.test(e.entityId.replace(/[._]/g, " ")) || SYSTEM_RE.test(e.friendlyName ?? "");

export function displayName(e: HaEntity): string {
  const words = entityName(e).split(/\s+/);
  const out: string[] = [];
  for (const w of words) if (!out.length || out[out.length - 1].toLowerCase() !== w.toLowerCase()) out.push(w);
  return out.join(" ");
}

export function groupOf(e: HaEntity): HomeGroupId {
  if (isSystemEntity(e)) return "system";
  switch (e.domain) {
    case "scene": case "script": return "scenes";
    case "light": return "lights";
    case "switch": case "fan": case "input_boolean": case "automation": return "switches";
    case "climate": return "climate";
    case "media_player": return "media";
    case "cover": case "lock": return "covers";
    case "sensor": case "binary_sensor": return "sensors";
    default: return "other";
  }
}

export function groupDevices(list: HaEntity[]): Array<{ id: HomeGroupId; title: string; devices: HaEntity[] }> {
  const by = new Map<HomeGroupId, HaEntity[]>();
  for (const e of list) { const g = groupOf(e); by.set(g, [...(by.get(g) ?? []), e]); }
  return ORDER.filter((g) => by.has(g)).map((g) => ({ id: g, title: GROUP_TITLE[g], devices: by.get(g)! }));
}
