import { describe, expect, it, vi } from "vitest";
import { agentsFacts, smartHomeFacts } from "./factsSurfaces";
import { runTake, type TakeDeps } from "./generate";
import type { KvDeps } from "./store";
import { validateTake } from "./validate";
import type { ActivityEntry, OutboxStatus } from "../types";
import type { HaDevice, HaDevicesResult } from "../bindings.gen";

const TODAY = "2026-11-19";
const NOW = Date.parse("2026-11-19T09:00:00+09:00");
const outbox = (o: Partial<OutboxStatus> = {}): OutboxStatus => ({ gateway_online: true, queue_depth: 0, last_delivered_at: null, last_error: null, total_delivered: 0, ...o });
const act = (startedAt: number, status = "ok"): ActivityEntry => ({ id: String(startedAt), type: "routine", name: "Health check", status, startedAt, completedAt: startedAt, durationMs: 5, detail: null, outcome: null });

describe("agentsFacts", () => {
  it("states Amara, the crew service, today's runs (Tokyo day) and delivery counts", () => {
    const set = agentsFacts({ amara: "online", crew: "operational", outbox: outbox(), activity: [act(NOW - 3_600_000), act(NOW - 7_200_000), act(NOW - 2 * 86_400_000)] }, TODAY)!;
    const by = (k: string) => set.facts.find((f) => f.key === k)!;
    expect(by("amara").display).toBe("online");
    expect(by("crew").display).toBe("online");
    expect(by("runsToday").value).toBe(2);
    expect(by("delivered").value).toBe(0);
    expect(by("waiting").value).toBe(0);
    expect(set.facts.find((f) => f.key === "failedToday")).toBeUndefined();
  });
  it("counts failed runs only when there are some, and says offline / not configured honestly", () => {
    const set = agentsFacts({ amara: "offline", crew: "not_configured", outbox: outbox({ total_delivered: 4, queue_depth: 2 }), activity: [act(NOW, "failed"), act(NOW - 1000)] }, TODAY)!;
    expect(set.facts.find((f) => f.key === "failedToday")!.value).toBe(1);
    expect(set.facts.find((f) => f.key === "amara")!.display).toBe("offline");
    expect(set.facts.find((f) => f.key === "crew")!.display).toBe("not configured");
    expect(set.facts.find((f) => f.key === "delivered")!.display).toBe("4");
  });
  it("a failed read is missing, never guessed; nothing readable returns null", () => {
    const set = agentsFacts({ amara: "online", crew: null, outbox: null, activity: null }, TODAY)!;
    expect(set.facts.map((f) => f.key)).toEqual(["amara"]);
    expect(agentsFacts({ amara: null, crew: null, outbox: null, activity: null }, TODAY)).toBeNull();
  });
  it("the fingerprint changes with a delivery, so the take refreshes", () => {
    const a = agentsFacts({ amara: "online", crew: null, outbox: outbox(), activity: [] }, TODAY)!;
    const b = agentsFacts({ amara: "online", crew: null, outbox: outbox({ total_delivered: 1 }), activity: [] }, TODAY)!;
    expect(a.fingerprint).not.toBe(b.fingerprint);
  });
  it("a take can quote delivery counts, rejects an invented one", () => {
    const set = agentsFacts({ amara: "online", crew: "operational", outbox: outbox({ total_delivered: 3 }), activity: [] }, TODAY)!;
    expect(validateTake("Amara is online. 3 items have been delivered. Want to check the queue?", set.facts).ok).toBe(true);
    expect(validateTake("Amara is online. 9 items have been delivered.", set.facts).ok).toBe(false);
  });
});

const dev = (o: Partial<HaDevice>): HaDevice => ({ entityId: "light.x", name: "SECRET NAME", deviceType: "light", room: "SECRET ROOM", state: "on", unit: null, system: false, controllable: true, services: [], brightnessPct: null, percentage: null, temperature: null, currentTemperature: null, minTemperature: null, maxTemperature: null, hvacModes: [], volume: null, ...o }) as HaDevice;
const list = (devices: HaDevice[]): HaDevicesResult => ({ devices, roomsAvailable: true }) as HaDevicesResult;

describe("smartHomeFacts", () => {
  it("counts household devices, lights on and devices with no reading; system entities are left out", () => {
    const set = smartHomeFacts(list([dev({}), dev({ state: "off" }), dev({ deviceType: "switch", state: null }), dev({ deviceType: "sensor", system: true, controllable: false })]), TODAY)!;
    const by = (k: string) => set.facts.find((f) => f.key === k)!;
    expect(by("devices").value).toBe(3);
    expect(by("controllable").value).toBe(3);
    expect(by("lights").value).toBe(2);
    expect(by("lightsOn").value).toBe(1);
    expect(by("noReading").value).toBe(1);
  });
  it("is null when HA is not connected, the list is empty or only system entities exist", () => {
    expect(smartHomeFacts(null, TODAY)).toBeNull();
    expect(smartHomeFacts(list([]), TODAY)).toBeNull();
    expect(smartHomeFacts(list([dev({ system: true })]), TODAY)).toBeNull();
  });
  it("no device or room name, entity id or state text reaches the facts or fingerprint", () => {
    const set = smartHomeFacts(list([dev({})]), TODAY)!;
    const text = JSON.stringify(set);
    expect(text).not.toMatch(/SECRET|light\.x/);
  });
});

function deps(facts: TakeDeps["facts"]) {
  const store = new Map<string, string>();
  const kv: KvDeps = { load: async (k) => store.get(k) ?? null, save: async (k, v) => { store.set(k, v); } };
  const ask = vi.fn(async () => ({ content: "Amara is online. 0 items delivered so far. Should I check the queue?", tier: "light" }));
  const log = vi.fn(async () => undefined);
  const d: TakeDeps = { now: () => NOW, kv, ask, facts, log, recent: () => [] };
  return { d, ask, log, store };
}

describe("runTake for agents and smarthome", () => {
  it("agents: generates once per day from facts, logs it, then serves the cache", async () => {
    const h = deps(async (_p, today) => agentsFacts({ amara: "online", crew: null, outbox: outbox(), activity: [] }, today));
    const r = await runTake("agents", h.d);
    expect(r).toMatchObject({ state: "live", value: { page: "agents", day: TODAY } });
    await runTake("agents", h.d);
    expect(h.ask).toHaveBeenCalledTimes(1);
    expect(h.log).toHaveBeenCalledTimes(1);
    expect([...h.store.keys()]).toEqual(["amara_take_agents"]);
  });
  it("smarthome: no devices means no router call and an unwired card", async () => {
    const h = deps(async (_p, today) => smartHomeFacts(list([]), today));
    expect(await runTake("smarthome", h.d)).toEqual({ state: "unwired" });
    expect(h.ask).not.toHaveBeenCalled();
  });
  it("smarthome: a rejected router reply falls back to a template take that cites a real count", async () => {
    const h = deps(async (_p, today) => smartHomeFacts(list([dev({}), dev({ state: "off" })]), today));
    h.ask.mockResolvedValue({ content: "Everything looks great!", tier: "light" });
    const r = await runTake("smarthome", h.d);
    expect(r).toMatchObject({ state: "live", value: { page: "smarthome" } });
    if (r.state === "live") expect(r.value.text).toContain("Household devices Home Assistant lists: 2");
  });
});
