import { describe, expect, it } from "vitest";
import { HOST_VIEWS, LANDING_ENTRIES, ROUTE_ALIASES, SECTION_ANCHORS, SYSTEM_GROUPS, SYSTEMS, TODAY_ROUTE, resolveRoute, screenFor } from "./routes";

describe("navigation (6 items, 3 groups)", () => {
  it("has the spec order and grouping", () => {
    expect(SYSTEM_GROUPS.map((g) => g.label)).toEqual(["", "Work", "System"]);
    expect(SYSTEMS.map((s) => s.id)).toEqual(["terrance", "chat", "mindbody", "business", "settings", "smarthome"]);
  });

  it("resolves every folded id to its host and back to the legacy screen", () => {
    expect(resolveRoute("health")).toEqual({ id: "mindbody", view: "fitness" });
    expect(resolveRoute("neuro")).toEqual({ id: "mindbody", view: "neuro" });
    expect(resolveRoute("sessions")).toEqual({ id: "mindbody", view: "neuro-history" });
    expect(HOST_VIEWS.mindbody?.map((v) => v.label)).toEqual(["Overview", "Fitness", "Neuro", "Neuro history", "Patterns"]);
    expect(HOST_VIEWS.mindbody?.[0]).toEqual({ view: "main", label: "Overview", screen: "mindbody" });
    expect(resolveRoute("trajectory")).toEqual({ id: "terrance", view: "goals" });
    expect(resolveRoute("revenue")).toEqual({ id: "business", view: "revenue" });
    expect(resolveRoute("assistant")).toEqual({ id: "chat", view: "activity" });
    expect(resolveRoute("crews")).toEqual({ id: "chat", view: "crews" });
    expect(HOST_VIEWS.chat?.map((v) => v.label)).toEqual(["Agents", "Crews"]);
    expect(resolveRoute("lists")).toEqual({ id: "terrance", view: "lists" });
    expect(resolveRoute("voice")).toEqual({ id: "settings", view: "voice" });
    expect(screenFor(resolveRoute("lists"))).toBe("terrance");
    expect(screenFor(resolveRoute("assistant"))).toBe("chat");
    expect(resolveRoute("devices")).toEqual({ id: "settings", view: "devices" });
    expect(screenFor(resolveRoute("trajectory"))).toBe("terrance");
    expect(screenFor(resolveRoute("devices"))).toBe("settings");
    // Section aliases (design-1110b B) render their host and scroll to the section.
    const sections: Record<string, string> = { trajectory: "terrance", devices: "settings", lists: "terrance", assistant: "chat", voice: "settings" };
    for (const [old, target] of Object.entries(ROUTE_ALIASES)) {
      if (sections[old]) {
        expect(screenFor(resolveRoute(old as never))).toBe(sections[old]);
        expect(SECTION_ANCHORS[target!.id]).toContain(target!.view);
        continue;
      }
      expect(screenFor(resolveRoute(old as never))).toBe(old);
      expect(HOST_VIEWS[target!.id]?.some((v) => v.view === target!.view)).toBe(true);
    }
  });

  it("labels Mind–Body with an en dash; Fitness and Neuro are its sub-views", () => {
    expect(SYSTEMS.map((s) => s.label)).toEqual(["Home", "Agents", "Mind–Body", "Business", "Settings", "Smart home"]);
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
    expect(LANDING_ENTRIES.map((e) => e.label)).toEqual(["Today", "Agents", "Mind–Body", "Business", "Settings", "Smart home"]);
    expect(LANDING_ENTRIES.map((e) => e.shortcut)).toEqual([null, 2, 3, 4, 5, 6]);
    expect(LANDING_ENTRIES.map((e) => e.icon)).toEqual(SYSTEMS.map((s) => s.icon));
  });
});
