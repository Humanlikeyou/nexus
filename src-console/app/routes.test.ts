import { describe, expect, it } from "vitest";
import { HOST_VIEWS, LANDING_ENTRIES, ROUTE_ALIASES, SYSTEM_GROUPS, SYSTEMS, TODAY_ROUTE, resolveRoute, screenFor } from "./routes";

describe("navigation (8 items, 4 groups)", () => {
  it("has the spec order and grouping", () => {
    expect(SYSTEM_GROUPS.map((g) => g.label)).toEqual(["", "Body & Mind", "Work", "System"]);
    expect(SYSTEMS.map((s) => s.id)).toEqual(["terrance", "chat", "health", "neuro", "mindbody", "business", "settings", "smarthome"]);
  });

  it("resolves every folded id to its host and back to the legacy screen", () => {
    expect(resolveRoute("sessions")).toEqual({ id: "neuro", view: "history" });
    expect(resolveRoute("trajectory")).toEqual({ id: "terrance", view: "goals" });
    expect(resolveRoute("revenue")).toEqual({ id: "business", view: "revenue" });
    expect(resolveRoute("assistant")).toEqual({ id: "chat", view: "activity" });
    expect(resolveRoute("crews")).toEqual({ id: "chat", view: "crews" });
    expect(HOST_VIEWS.chat?.map((v) => v.label)).toEqual(["Chat", "Activity", "Crews", "Voice"]);
    expect(resolveRoute("devices")).toEqual({ id: "settings", view: "devices" });
    expect(screenFor(resolveRoute("trajectory"))).toBe("terrance");
    expect(screenFor(resolveRoute("devices"))).toBe("settings");
    for (const [old, target] of Object.entries(ROUTE_ALIASES)) {
      if (old === "trajectory" || old === "devices") continue;
      expect(screenFor(resolveRoute(old as never))).toBe(old);
      expect(HOST_VIEWS[target!.id]?.some((v) => v.view === target!.view)).toBe(true);
    }
  });

  it("labels Mind–Body with an en dash and keeps existing labels", () => {
    expect(SYSTEMS.map((s) => s.label)).toEqual(["Home", "Chat & Agents", "Fitness", "Neuro", "Mind–Body", "Business", "Settings", "Smart home"]);
  });

  it("keeps host ids and their main views", () => {
    for (const s of SYSTEMS) {
      expect(resolveRoute(s.id)).toEqual({ id: s.id, view: "main" });
      expect(screenFor(resolveRoute(s.id))).toBe(s.id === "terrance" ? "landing" : s.id);
    }
  });

  it("Home is the landing page; the dashboard stays reachable as Today and through #goals", () => {
    expect(screenFor({ id: "terrance", view: "main" })).toBe("landing");
    expect(screenFor(TODAY_ROUTE)).toBe("terrance");
    expect(screenFor(resolveRoute("trajectory"))).toBe("terrance");
    expect(resolveRoute("landing")).toEqual({ id: "terrance", view: "main" });
  });

  it("landing buttons are Today plus every other section, with the sidebar's shortcuts", () => {
    expect(LANDING_ENTRIES.map((e) => e.label)).toEqual(["Today", "Chat & Agents", "Fitness", "Neuro", "Mind–Body", "Business", "Settings", "Smart home"]);
    expect(LANDING_ENTRIES.map((e) => e.shortcut)).toEqual([null, 2, 3, 4, 5, 6, 7, 8]);
    expect(LANDING_ENTRIES.map((e) => e.icon)).toEqual(SYSTEMS.map((s) => s.icon));
  });
});
