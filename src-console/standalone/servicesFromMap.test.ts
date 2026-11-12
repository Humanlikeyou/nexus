import { describe, expect, it } from "vitest";
import { nodeState, servicesFromMap } from "./servicesFromMap";
import type { MapPath, SystemMap } from "./systemMap";

const p = (id: string, from: string, to: string, status: MapPath["status"], reason: string | null = null): MapPath => ({ id, from, to, carries: `${id} traffic`, status, lastOkAt: null, reason, fix: null });
const map: SystemMap = {
  checkedAt: "2026-11-12T10:00:00+09:00",
  nodes: [
    { id: "app", label: "Amara OS", kind: "app" }, { id: "router", label: "Amara Router", kind: "backend" }, { id: "crew", label: "Crews", kind: "service" },
    { id: "muse", label: "Muse headset", kind: "device" }, { id: "tg", label: "Telegram", kind: "external" }, { id: "lone", label: "Lonely", kind: "store" },
  ],
  paths: [p("a-r", "app", "router", "ok"), p("a-c", "app", "crew", "broken", "The crew service isn't answering"), p("m-a", "muse", "app", "unknown"), p("a-t", "app", "tg", "off")],
};

describe("servicesFromMap", () => {
  it("one row per backend/service/store/device node; app and external skipped", () => {
    const { services } = servicesFromMap(map);
    expect(services.map((s) => s.id)).toEqual(["router", "crew", "muse", "lone"]);
  });
  it("derives state from paths and never invents meta", () => {
    const by = Object.fromEntries(servicesFromMap(map).services.map((s) => [s.id, s]));
    expect(by.router.state).toBe("healthy");
    expect(by.crew).toMatchObject({ state: "failed", what: "The crew service isn't answering", meta: "" });
    expect(by.muse.state).toBe("waiting");
    expect(by.lone.state).toBe("waiting");
    expect(JSON.stringify(by)).not.toMatch(/\bup \d|\d+ (days|GB|documents|memories)/);
  });
  it("nodeState rules", () => {
    expect(nodeState(["ok", "ok"])).toBe("healthy");
    expect(nodeState(["ok", "broken"])).toBe("failed");
    expect(nodeState(["off", "off"])).toBe("stopped");
    expect(nodeState(["ok", "off"])).toBe("waiting");
    expect(nodeState([])).toBe("waiting");
  });
});
