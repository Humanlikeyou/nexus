import type { ActivityEntry, OutboxStatus } from "../types";
import type { HaDevicesResult } from "../bindings.gen";
import { tokyoDay } from "../lib/mindBody";
import { fingerprintOf, makeFact } from "./facts";
import type { Fact, FactSet } from "./types";

/**
 * Facts for the Agents and Smart home pages. Counts and status words only: never an entity id, a device
 * or room name, a run name, an error body or a reading. A status fact has no number to quote (`accept` is
 * empty), so it informs the sentence without being a citable chip.
 */

const status = (key: string, label: string, display: string, value: number): Fact => ({ key, label, value, unit: "", display, baseline: null, accept: [] });

const finish = (page: "agents" | "smarthome", day: string, facts: Array<Fact | null>): FactSet | null => {
  const real = facts.filter((f): f is Fact => f !== null);
  return real.length ? { page, day, facts: real, fingerprint: fingerprintOf(real) } : null;
};

export type AgentsFactInput = {
  /** hermes.status from get_terrence_runtime_health; null when that read failed. */
  amara: string | null;
  /** crew_health raw text; null when that read failed. */
  crew: string | null;
  outbox: OutboxStatus | null;
  activity: ActivityEntry[] | null;
};

const FAILED = /fail|error/i;

/** Agents: Amara's runtime state, the crew service, today's recorded runs (Asia/Tokyo day) and delivery counts. Nothing readable, nothing returned. */
export function agentsFacts(input: AgentsFactInput, today: string): FactSet | null {
  const { amara, crew, outbox, activity } = input;
  const crewState = crew === null ? null : crew === "not_configured" ? { d: "not configured", v: 2 } : crew === "operational" ? { d: "online", v: 1 } : { d: "offline", v: 0 };
  const todays = activity ? activity.filter((a) => Number.isFinite(a.startedAt) && tokyoDay(new Date(a.startedAt).toISOString()) === today) : null;
  const failed = todays ? todays.filter((a) => FAILED.test(a.status)).length : 0;
  return finish("agents", today, [
    amara === null ? null : status("amara", "Amara status", amara === "online" ? "online" : "offline", amara === "online" ? 1 : 0),
    crewState ? status("crew", "Crew service", crewState.d, crewState.v) : null,
    todays ? makeFact({ key: "runsToday", label: "Routine and health-check runs recorded today", unit: "" }, todays.length) : null,
    todays && failed > 0 ? makeFact({ key: "failedToday", label: "Of those, failed today", unit: "" }, failed) : null,
    outbox ? makeFact({ key: "delivered", label: "Items delivered so far", unit: "" }, outbox.total_delivered) : null,
    outbox ? makeFact({ key: "waiting", label: "Items waiting to send", unit: "" }, outbox.queue_depth) : null,
  ]);
}

/**
 * Smart home: counts from the device list. Null when Home Assistant gave no list or the list has no
 * household device (backup and system entities do not count), so the card stays hidden rather than guess.
 */
export function smartHomeFacts(result: HaDevicesResult | null, today: string): FactSet | null {
  if (!result) return null;
  const devices = result.devices.filter((d) => !d.system);
  if (!devices.length) return null;
  const lights = devices.filter((d) => d.deviceType === "light");
  const lightsOn = lights.filter((d) => d.state === "on").length;
  const noReading = devices.filter((d) => d.state === null || d.state === undefined).length;
  return finish("smarthome", today, [
    makeFact({ key: "devices", label: "Household devices Home Assistant lists", unit: "" }, devices.length),
    makeFact({ key: "controllable", label: "Devices Amara can control", unit: "" }, devices.filter((d) => d.controllable).length),
    lights.length ? makeFact({ key: "lights", label: "Lights", unit: "" }, lights.length) : null,
    lights.length ? makeFact({ key: "lightsOn", label: "Lights on right now", unit: "" }, lightsOn) : null,
    noReading > 0 ? makeFact({ key: "noReading", label: "Devices with no reading", unit: "" }, noReading) : null,
  ]);
}
