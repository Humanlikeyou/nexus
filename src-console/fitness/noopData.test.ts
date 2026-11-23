import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { SAMPLE_ADAPTERS_KEY } from "../lib/sampleAdapters";
import { failureState, loadHypnogram, loadTrends, loadWorkouts } from "./noopData";

const sampleOn = () => localStorage.setItem(SAMPLE_ADAPTERS_KEY, "1");
beforeEach(() => { invoke.mockReset(); localStorage.removeItem(SAMPLE_ADAPTERS_KEY); vi.spyOn(console, "warn").mockImplementation(() => {}); });

describe("failureState", () => {
  it("a missing store is unwired; anything else is an error", () => {
    expect(failureState("NOOP has not created its local health store yet. Open NOOP, pair...")).toBe("unwired");
    expect(failureState({ kind: "binary_missing", detail: "NOOP isn't installed" })).toBe("unwired");
    expect(failureState("NOOP workouts could not be read: disk I/O")).toBe("error");
  });
});

describe("loadWorkouts", () => {
  it("live rows, asks for the chosen days", async () => {
    invoke.mockResolvedValue([{ day: "2026-11-10" }]);
    expect((await loadWorkouts(30)).state).toBe("live");
    expect(invoke).toHaveBeenCalledWith("noop_recent_workouts", { days: 30 }, undefined);
  });
  it("no rows is empty", async () => { invoke.mockResolvedValue([]); expect(await loadWorkouts(7)).toEqual({ state: "empty" }); });
  it("no store is unwired, a failed read is an error, a bad payload is an error", async () => {
    invoke.mockRejectedValueOnce("NOOP has not created its local health store yet.");
    expect(await loadWorkouts(7)).toEqual({ state: "unwired" });
    invoke.mockRejectedValueOnce("NOOP store could not be opened: x");
    expect(await loadWorkouts(7)).toEqual({ state: "error" });
    invoke.mockResolvedValueOnce({ nope: 1 });
    expect(await loadWorkouts(7)).toEqual({ state: "error" });
  });
  it("sample adapters are opt-in and never call the backend", async () => {
    sampleOn();
    const r = await loadWorkouts(14);
    expect(r.state).toBe("sample");
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("loadHypnogram", () => {
  it("live, with the day passed through (null for the latest night)", async () => {
    invoke.mockResolvedValue({ day: "2026-11-10", startedAt: "", endedAt: "", segments: [{ start: 1, end: 2, stage: "deep" }] });
    expect((await loadHypnogram("2026-11-10")).state).toBe("live");
    expect(invoke).toHaveBeenCalledWith("noop_hypnogram", { day: "2026-11-10" }, undefined);
    await loadHypnogram();
    expect(invoke).toHaveBeenLastCalledWith("noop_hypnogram", { day: null }, undefined);
  });
  it("null or no segments is empty; failures map like workouts", async () => {
    invoke.mockResolvedValueOnce(null);
    expect(await loadHypnogram()).toEqual({ state: "empty" });
    invoke.mockResolvedValueOnce({ day: "d", startedAt: "", endedAt: "", segments: [] });
    expect(await loadHypnogram()).toEqual({ state: "empty" });
    invoke.mockRejectedValueOnce("boom");
    expect(await loadHypnogram()).toEqual({ state: "error" });
  });
  it("sample is tagged sample", async () => { sampleOn(); expect((await loadHypnogram()).state).toBe("sample"); });
});

const series = (key: string) => ({ key, points: [{ day: "2026-11-09", value: 1 }, { day: "2026-11-10", value: 2 }] });
describe("loadTrends", () => {
  it("reads all 9 keys over a year, once", async () => {
    invoke.mockImplementation(async (_c: string, a: { args: { key: string } }) => series(a.args.key));
    const r = await loadTrends();
    expect(r.state).toBe("live");
    expect(invoke).toHaveBeenCalledTimes(9);
    expect(invoke).toHaveBeenCalledWith("noop_query", { tool: "metric_series", args: { key: "active_kcal", days: 365 } }, undefined);
  });
  it("one key failing or empty doesn't hide the rest", async () => {
    invoke.mockImplementation(async (_c: string, a: { args: { key: string } }) => {
      if (a.args.key === "spo2") throw { kind: "bad_output", detail: "x" };
      if (a.args.key === "steps") return { key: "steps", points: [] };
      if (a.args.key === "rhr") return { nope: true };
      return series(a.args.key);
    });
    const r = await loadTrends();
    if (r.state !== "live") throw new Error("expected live");
    expect(r.value.spo2.status).toBe("error");
    expect(r.value.rhr.status).toBe("error");
    expect(r.value.steps.status).toBe("empty");
    expect(r.value.hrv.status).toBe("ready");
  });
  it("helper not set up is unwired; every key erroring otherwise is an error; every key empty is empty", async () => {
    invoke.mockRejectedValue({ kind: "binary_missing", detail: "x" });
    expect(await loadTrends()).toEqual({ state: "unwired" });
    invoke.mockReset(); invoke.mockRejectedValue({ kind: "timeout", detail: "x" });
    expect(await loadTrends()).toEqual({ state: "error" });
    invoke.mockReset(); invoke.mockImplementation(async (_c: string, a: { args: { key: string } }) => ({ key: a.args.key, points: [] }));
    expect(await loadTrends()).toEqual({ state: "empty" });
  });
  it("sample adapters give 9 ready series and make no calls", async () => {
    sampleOn();
    const r = await loadTrends();
    if (r.state !== "sample") throw new Error("expected sample");
    expect(Object.keys(r.value)).toHaveLength(9);
    expect(invoke).not.toHaveBeenCalled();
  });
});
