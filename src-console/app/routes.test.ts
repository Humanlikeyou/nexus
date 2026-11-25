import { describe, expect, it } from "vitest";
import { HOST_VIEWS, LANDING_ENTRIES, ROUTE_ALIASES, SECTION_ANCHORS, SYSTEM_GROUPS, SYSTEMS, TODAY_ROUTE, VIEW_REDIRECTS, canonicalRoute, resolveRoute, screenFor, tabOf, ventureOf, ventureRoute } from "./routes";

describe("navigation (7 items, 2 groups)", () => {
  it("has the spec order and grouping (decision 0009: Home, Life, Mind–Body, Enterprise, Agents · System: Smart home, Settings)", () => {
    expect(SYSTEM_GROUPS.map((g) => g.label)).toEqual(["", "System"]);
    expect(SYSTEMS.map((s) => s.id)).toEqual(["terrance", "life", "mindbody", "enterprise", "chat", "smarthome", "settings"]);
  });

  it("resolves every folded id to its host and back to the legacy screen", () => {
    expect(resolveRoute("health")).toEqual({ id: "mindbody", view: "fitness" });
    expect(resolveRoute("neuro")).toEqual({ id: "mindbody", view: "neuro" });
    expect(resolveRoute("sessions")).toEqual({ id: "mindbody", view: "neuro-history" });
    expect(HOST_VIEWS.mindbody?.map((v) => v.label)).toEqual(["Overview", "Fitness", "Neuro", "Neuro history", "Patterns"]);
    expect(HOST_VIEWS.mindbody?.[0]).toEqual({ view: "main", label: "Overview", screen: "mindbody" });
    expect(resolveRoute("trajectory")).toEqual({ id: "life", view: "goals" });
    expect(resolveRoute("revenue")).toEqual({ id: "enterprise", view: "finances" });
    expect(resolveRoute("assistant")).toEqual({ id: "chat", view: "activity" });
    expect(resolveRoute("crews")).toEqual({ id: "chat", view: "crews" });
    expect(HOST_VIEWS.chat?.map((v) => v.label)).toEqual(["Agents", "Crews"]);
    expect(resolveRoute("lists")).toEqual({ id: "terrance", view: "lists" });
    expect(resolveRoute("voice")).toEqual({ id: "settings", view: "voice" });
    expect(screenFor(resolveRoute("lists"))).toBe("terrance");
    expect(screenFor(resolveRoute("assistant"))).toBe("chat");
    expect(resolveRoute("devices")).toEqual({ id: "settings", view: "devices" });
    expect(screenFor(resolveRoute("trajectory"))).toBe("life");
    expect(screenFor(resolveRoute("revenue"))).toBe("revenue");
    expect(screenFor(resolveRoute("devices"))).toBe("settings");
    const sections: Record<string, string> = { lists: "terrance", assistant: "chat" };
    for (const [old, target] of Object.entries(ROUTE_ALIASES)) {
      const resolved = resolveRoute(old as never);
      if (sections[old]) {
        expect(screenFor(resolved)).toBe(sections[old]);
        expect(SECTION_ANCHORS[target!.id]).toContain(target!.view);
        continue;
      }
      // Every other alias lands on a real tab of a host.
      expect(HOST_VIEWS[resolved.id]?.some((v) => v.view === resolved.view)).toBe(true);
    }
  });

  it("labels Mind–Body with an en dash; Life and Enterprise follow it", () => {
    expect(SYSTEMS.map((s) => s.label)).toEqual(["Home", "Life", "Mind–Body", "Enterprise", "Agents", "Smart home", "Settings"]);
  });

  it("keeps host ids; Life opens on Overview (decision 0056) and Enterprise on Overview", () => {
    for (const s of SYSTEMS) {
      const r = resolveRoute(s.id);
      expect(r).toEqual({ id: s.id, view: s.id === "life" ? "overview" : "main" });
    }
    expect(screenFor(resolveRoute("life"))).toBe("life");
    expect(screenFor(resolveRoute("enterprise"))).toBe("business");
    expect(screenFor(resolveRoute("terrance"))).toBe("landing");
  });

  it("Home is the landing page; the dashboard stays reachable as Today", () => {
    expect(screenFor({ id: "terrance", view: "main" })).toBe("landing");
    expect(screenFor(TODAY_ROUTE)).toBe("terrance");
    expect(resolveRoute("landing")).toEqual({ id: "terrance", view: "main" });
  });

  it("landing buttons are Today plus every other section, with the sidebar's shortcuts", () => {
    expect(LANDING_ENTRIES.map((e) => e.label)).toEqual(["Today", "Life", "Mind–Body", "Enterprise", "Agents", "Smart home", "Settings"]);
    expect(LANDING_ENTRIES.map((e) => e.shortcut)).toEqual([null, 2, 3, 4, 5, 6, 7]);
    expect(LANDING_ENTRIES.map((e) => e.icon)).toEqual(SYSTEMS.map((s) => s.icon));
  });
});

describe("design-1111v redirects (HANDOFF-1111v §2): old routes keep working", () => {
  const PAIRS: Array<[{ id: never; view: string }, { id: string; view: string }]> = [
    [{ id: "terrance" as never, view: "goals" }, { id: "life", view: "goals" }],
    [{ id: "trajectory" as never, view: "main" }, { id: "life", view: "goals" }],
    [{ id: "terrance" as never, view: "weekly-review" }, { id: "life", view: "weekly-review" }],
    [{ id: "terrance" as never, view: "life-admin" }, { id: "life", view: "dates" }],
    [{ id: "business" as never, view: "personal" }, { id: "life", view: "money" }],
    [{ id: "business" as never, view: "revenue" }, { id: "enterprise", view: "finances" }],
    [{ id: "revenue" as never, view: "main" }, { id: "enterprise", view: "finances" }],
    [{ id: "business" as never, view: "work" }, { id: "enterprise", view: "tasks" }],
    [{ id: "business" as never, view: "main" }, { id: "enterprise", view: "main" }],
    [{ id: "business" as never, view: "ventures" }, { id: "enterprise", view: "ventures" }],
    [{ id: "business" as never, view: "decisions" }, { id: "enterprise", view: "decisions" }],
    [{ id: "business" as never, view: "content" }, { id: "enterprise", view: "content" }],
    [{ id: "business" as never, view: "ventures/taiyo-skin" }, { id: "enterprise", view: "ventures/taiyo-skin" }],
    [{ id: "lists" as never, view: "main" }, { id: "terrance", view: "lists" }],
    [{ id: "life" as never, view: "main" }, { id: "life", view: "overview" }],
  ];
  it.each(PAIRS)("%o → %o", (from, to) => {
    expect(resolveRoute(from.id, from.view)).toEqual(to);
    expect(screenFor(resolveRoute(from.id, from.view))).toBe(screenFor(to as never));
  });

  it("canonicalRoute is what navigate() applies, and it is idempotent", () => {
    for (const [k, to] of Object.entries(VIEW_REDIRECTS)) {
      const [id, view] = k.split(":");
      expect(canonicalRoute({ id: id as never, view })).toEqual(to);
      expect(canonicalRoute(to)).toEqual(to);
    }
  });

  it("Today keeps only its own anchors", () => {
    expect(SECTION_ANCHORS.terrance).toEqual(["needs-you", "checkin", "lists"]);
  });

  it("a venture is its own route on the Ventures tab", () => {
    const r = ventureRoute("taiyo-skin");
    expect(r).toEqual({ id: "enterprise", view: "ventures/taiyo-skin" });
    expect(tabOf(r)).toBe("ventures");
    expect(screenFor(r)).toBe("business");
    expect(ventureOf(r)).toBe("taiyo-skin");
    expect(ventureOf({ id: "enterprise", view: "ventures" })).toBeNull();
  });

  it("the tabs keep Taisun's names", () => {
    expect(HOST_VIEWS.life?.map((v) => v.label)).toEqual(["Overview", "Goals & projects", "Habits & routines", "Weekly review", "Dates & deadlines", "Personal money"]);
    expect(HOST_VIEWS.enterprise?.map((v) => v.label)).toEqual(["Overview", "Decisions", "Ventures", "Finances", "Treasury", "Tasks", "Content"]);
    expect(HOST_VIEWS.business).toBeUndefined();
  });
});
