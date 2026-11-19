import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { HaEntity } from "../../lib/homeAssistant";
import { stateLabel } from "../../lib/homeAssistant";
import { deliveryLine, groupPulse } from "../Agents";
import { displayName, groupDevices, groupOf, isSystemEntity } from "../homeGroups";
import { PREVIEW_NOTES, PageTake, type TakeSurface } from "../PageTake";
import { anonymousStations, glanceOf, pathLabel, pathPhase, stationsOf, strandPath } from "../pathModel";
// @ts-expect-error vite ?raw import
import todaySrc from "../../personal/TodaySections.tsx?raw";
// @ts-expect-error vite ?raw import
import neuroSrc from "../../screens/NeuroScreen.tsx?raw";
// @ts-expect-error vite ?raw import
import venturesSrc from "../../screens/BusinessPanels.tsx?raw";
// @ts-expect-error vite ?raw import
import revenueSrc from "../../screens/RevenueScreen.tsx?raw";
// @ts-expect-error vite ?raw import
import contentSrc from "../../screens/EnterprisePages.tsx?raw";
// @ts-expect-error vite ?raw import
import agentsSrc from "../../screens/AgentsScreen.tsx?raw";
// @ts-expect-error vite ?raw import
import homeSrc from "../../screens/SmartHomeScreen.tsx?raw";

const ent = (entityId: string, extra: Partial<HaEntity> = {}): HaEntity => ({ entityId, state: "on", domain: entityId.split(".")[0], attributes: {}, ...extra });

describe("design-1118a: Amara's take card on every page", () => {
  afterEach(() => { cleanup(); localStorage.removeItem("nexus.sampleAdapters"); });

  it("every redesigned page renders a take card under its hero", () => {
    const pages: Array<[string, string, RegExp]> = [
      ["Life › Goals", todaySrc as string, /<PageTake surface="goals"/],
      ["Neuro (incl. Hemi-Sync)", neuroSrc as string, /<AmaraTake page="neuro"/],
      ["Ventures / Trading", venturesSrc as string, /<PageTake surface="ventures"/],
      ["Finances", revenueSrc as string, /<PageTake surface="finances"/],
      ["Content", contentSrc as string, /<PageTake surface="content"/],
      ["Agents", agentsSrc as string, /<PageTake surface="agents"/],
      ["Smart home", homeSrc as string, /<PageTake surface="smarthome"/],
    ];
    for (const [name, src, re] of pages) expect(src, name).toMatch(re);
  });

  it("Agents and Smart home use their live generators: hidden once unwired, Sample data note only in previews", async () => {
    for (const s of ["agents", "smarthome"] as TakeSurface[]) {
      const { container, unmount } = render(<PageTake surface={s} />);
      await waitFor(() => expect(container.textContent).toBe("")); // app, no data readable: hidden until a take is written
      unmount();
      localStorage.setItem("nexus.sampleAdapters", "1");
      render(<PageTake surface={s} />);
      expect(screen.getByText(PREVIEW_NOTES[s].text)).toBeInTheDocument();
      expect(screen.getAllByText(/Sample data/).length).toBeGreaterThan(0);
      cleanup(); localStorage.removeItem("nexus.sampleAdapters");
    }
  });
});

describe("design-1118a Agents delivery truth", () => {
  const ob = (o: Partial<{ queue_depth: number; total_delivered: number; last_delivered_at: number | null }>) => ({ gateway_online: true, queue_depth: 0, last_delivered_at: null, last_error: null, total_delivered: 0, ...o }) as never;
  it("never says All delivered when nothing ever was", () => {
    expect(deliveryLine(ob({}), false)).toBe("No deliveries yet");
    expect(deliveryLine(ob({ total_delivered: 3, last_delivered_at: 1 }), false)).toBe("All delivered");
    expect(deliveryLine(ob({ queue_depth: 2 }), false)).toBe("2 waiting to send");
    expect(deliveryLine(ob({}), true)).toBe("Delivery is out of reach");
  });
  it("groups today's health checks into one line", () => {
    const now = new Date(2026, 10, 18, 15, 0).getTime();
    const a = (name: string, h: number, status = "ok") => ({ name, type: "routine", status, startedAt: new Date(2026, 10, 18, h).getTime() }) as never;
    const p = groupPulse([a("Health check", 9), a("health check", 12, "failed"), a("Digest", 10), { ...(a("Health check", 9) as object), startedAt: new Date(2026, 10, 17, 9).getTime() } as never], now);
    expect(p.checks).toHaveLength(2);
    expect(p.failed).toHaveLength(1);
  });
});

describe("design-1118a Smart home grouping", () => {
  it("dedupes doubled words (no 'Backup Backup')", () => {
    expect(displayName(ent("sensor.backup_backup_manager", { friendlyName: "Backup Backup manager" }))).toBe("Backup manager");
  });
  it("puts backup / supervisor sensors in System, never with household devices", () => {
    expect(isSystemEntity(ent("sensor.backup_last_attempt"))).toBe(true);
    expect(groupOf(ent("sensor.backup_last_attempt"))).toBe("system");
    expect(groupOf(ent("light.kitchen"))).toBe("lights");
    expect(groupOf(ent("scene.evening"))).toBe("scenes");
    expect(groupOf(ent("climate.living"))).toBe("climate");
    const g = groupDevices([ent("sensor.backup_next"), ent("light.desk"), ent("scene.movie"), ent("sensor.temp")]);
    expect(g.map((x) => x.id)).toEqual(["scenes", "lights", "sensors", "system"]);
  });
  it("shows 'No reading' instead of Unknown", () => {
    expect(stateLabel(ent("sensor.x", { state: "unknown" }))).toBe("No reading");
    expect(stateLabel(ent("sensor.x", { state: "" }))).toBe("No reading");
  });
});

describe("design-1118a path model", () => {
  const rm = (steps: Array<{ done: boolean }>, status = "active", pct = 25) => ({ status, pct, steps: steps.map((s, i) => ({ id: `s${i}`, title: `Step ${i + 1}`, size: "medium", order: i, done: s.done })) }) as never;
  it("marks done / current / next and labels where he is", () => {
    const r = rm([{ done: true }, { done: false }, { done: false }, { done: false }]);
    const st = stationsOf((r as { steps: never[] }).steps);
    expect(st.map((s) => s.state)).toEqual(["done", "current", "next", "upcoming"]);
    const g = glanceOf(r, st);
    expect(g.text).toBe("25% · 1 of 4");
    expect(pathLabel("Taiyo Skin", g, st)).toBe("Taiyo Skin: 25% · 1 of 4. You are here: Step 2.");
  });
  it("a drafting roadmap is 'planning', unwired stays unwired, and counts never invent names", () => {
    expect(pathPhase(rm([], "drafting"), false)).toBe("planning");
    expect(pathPhase(null, true)).toBe("unwired");
    expect(anonymousStations(3, 1).every((s) => s.title === null)).toBe(true);
    expect(glanceOf(null, []).text).toBe("No route yet");
  });
  it("the background strands tile seamlessly (periodic)", () => {
    const d = strandPath(0, 680, 120, 0);
    expect(d.startsWith("M")).toBe(true);
    const ys = (s: string) => s.match(/-?\d+(\.\d+)?/g)!.map(Number);
    const n = ys(d);
    expect(n[1]).toBeCloseTo(n[n.length - 1], 1); // same height at both ends
  });
});
