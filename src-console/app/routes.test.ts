import { describe, expect, it } from "vitest";
import { HOST_VIEWS, ROUTE_ALIASES, SYSTEM_GROUPS, SYSTEMS, resolveRoute, screenFor } from "./routes";

describe("navigation (6 items, 4 groups)", () => {
  it("has the spec order and grouping", () => {
    expect(SYSTEM_GROUPS.map((g) => g.label)).toEqual(["", "Body & Mind", "Work", "System"]);
    expect(SYSTEMS.map((s) => s.id)).toEqual(["terrance", "chat", "health", "neuro", "business", "settings"]);
  });

  it("resolves every folded id to its host and back to the legacy screen", () => {
    expect(resolveRoute("sessions")).toEqual({ id: "neuro", view: "history" });
    expect(resolveRoute("trajectory")).toEqual({ id: "terrance", view: "goals" });
    expect(resolveRoute("revenue")).toEqual({ id: "business", view: "revenue" });
    expect(resolveRoute("assistant")).toEqual({ id: "chat", view: "activity" });
    expect(resolveRoute("devices")).toEqual({ id: "settings", view: "devices" });
    expect(screenFor(resolveRoute("trajectory"))).toBe("terrance");
    for (const [old, target] of Object.entries(ROUTE_ALIASES)) {
      if (old === "trajectory") continue;
      expect(screenFor(resolveRoute(old as never))).toBe(old);
      expect(HOST_VIEWS[target!.id]?.some((v) => v.view === target!.view)).toBe(true);
    }
  });

  it("keeps host ids and their main views", () => {
    for (const s of SYSTEMS) {
      expect(resolveRoute(s.id)).toEqual({ id: s.id, view: "main" });
      expect(screenFor(resolveRoute(s.id))).toBe(s.id);
    }
  });
});
