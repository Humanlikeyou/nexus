import { afterEach, describe, expect, it, vi } from "vitest";
import { makeHomeVitals, recoveryFrom, ringDayKeys, ringsFrom } from "./homeVitals";

const NOW = new Date("2026-11-19T03:00:00Z"); // 12:00 JST
const metric = (day: string, recovery: number | null) => ({ day, source: "noop", recovery });
afterEach(() => { delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; });
const inApp = () => { (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}; };

describe("recoveryFrom", () => {
  it("is today's score, or an empty reason; never an older day", () => {
    expect(recoveryFrom({ today: metric("2026-11-19", 61) } as never, "2026-11-19", false)).toEqual({ kind: "score", day: "2026-11-19", percent: 61 });
    expect(recoveryFrom({ today: metric("2026-11-18", 80) } as never, "2026-11-19", false)).toEqual({ kind: "empty", reason: "pending" });
    expect(recoveryFrom({ today: metric("2026-11-19", null) } as never, "2026-11-19", true)).toEqual({ kind: "empty", reason: "hr_only" });
    expect(recoveryFrom({ today: null } as never, "2026-11-19", false)).toEqual({ kind: "empty", reason: "pending" });
    expect(recoveryFrom(null, "2026-11-19", false)).toEqual({ kind: "empty", reason: "unavailable" });
  });
});

describe("rings", () => {
  it("ringDayKeys counts back from today across a month edge", () => {
    expect(ringDayKeys("2026-12-02", 4)).toEqual(["2026-11-29", "2026-11-30", "2026-12-01", "2026-12-02"]);
  });
  it("keeps null as null (never 0) and marks days with no row", () => {
    const rows = [{ day: "2026-11-18", recovery: 0, sleepTotalMin: null, strain: 12.5, museMin: null }];
    expect(ringsFrom({ rows } as never, ["2026-11-17", "2026-11-18"])).toEqual([
      { day: "2026-11-17", recovery: null, sleepMin: null, strain: null, museMin: null },
      { day: "2026-11-18", recovery: 0, sleepMin: null, strain: 12.5, museMin: null },
    ]);
  });
});

describe("makeHomeVitals", () => {
  it("is unwired outside the app and never calls the bridge", async () => {
    const call = vi.fn();
    const v = makeHomeVitals({ call: call as never, now: () => NOW });
    expect((await v.recovery()).state).toBe("unwired");
    expect((await v.rings()).state).toBe("unwired");
    expect(call).not.toHaveBeenCalled();
  });
  it("reads the existing commands: snapshot (+ needs_you only when there is no score) and daily_range", async () => {
    inApp();
    const calls: [string, unknown][] = [];
    const call = vi.fn(async (c: string, a?: unknown) => {
      calls.push([c, a]);
      if (c === "get_noop_health_snapshot") return { today: metric("2026-11-19", null) };
      if (c === "needs_you_list") return [{ source: "strap", state: "ok", rows: [{ id: "strap-hr-only" }] }];
      return { rows: [{ day: "2026-11-19", recovery: 40, sleepTotalMin: 400, strain: null, museMin: null }] };
    }) as never;
    const v = makeHomeVitals({ call, now: () => NOW });
    const r = await v.recovery();
    expect(r.state === "live" && r.value).toEqual({ kind: "empty", reason: "hr_only" });
    const g = await v.rings(3);
    expect(g.state === "live" && g.value.map((d) => d.day)).toEqual(["2026-11-17", "2026-11-18", "2026-11-19"]);
    expect(g.state === "live" && g.value[2]).toEqual({ day: "2026-11-19", recovery: 40, sleepMin: 400, strain: null, museMin: null });
    expect(calls.map(([c]) => c)).toEqual(["get_noop_health_snapshot", "needs_you_list", "daily_range"]);
    expect(calls[2][1]).toEqual({ from: "2026-11-17", to: "2026-11-19" });
  });
  it("a score skips the needs-you read; a failed read is unwired", async () => {
    inApp();
    const call = vi.fn(async (c: string) => { if (c === "get_noop_health_snapshot") return { today: metric("2026-11-19", 70) }; throw new Error("x"); }) as never;
    const v = makeHomeVitals({ call, now: () => NOW });
    const r = await v.recovery();
    expect(r.state === "live" && r.value).toEqual({ kind: "score", day: "2026-11-19", percent: 70 });
    expect((await v.rings()).state).toBe("unwired");
  });
});
