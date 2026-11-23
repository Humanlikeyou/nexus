import { describe, expect, it, vi } from "vitest";
import type { CrewInfo, CrewRunSteps } from "../types";
import { sampleCrews, unwiredCrews } from "./adapters";
import { liveRunAdapter, loadRunDetail, mapMetrics, mapSteps, clearRunDetails, getRunDetail } from "./runDetail";

const run: CrewInfo = { id: "r1", name: "n", status: "completed", agents: ["researcher", "analyst"], started_at: "2026-01-01T00:00:00Z", task: "t", result: "ok",
  finished_at: "2026-01-01T00:00:48Z", tokens: 3210, requests: 4, cost_usd: 0 };
const detail: CrewRunSteps = {
  steps: [
    { id: 1, agent: "researcher", kind: "task", label: "researcher: task", start_ms: 0, end_ms: 20000, status: "ok" },
    { id: 2, agent: "analyst", kind: "task", label: "analyst: task", start_ms: 20000, end_ms: 40000, status: "failed", error: "x" },
    { id: 3, agent: "analyst", kind: "task", label: "tool: kb_query", start_ms: 40000, end_ms: 40000, status: "skipped" },
  ],
  handoffs: [{ from: "researcher", to: "analyst", status: "ok" }],
};

describe("crew run detail adapter", () => {
  it("maps a two-role run to a timeline, one handoff and metrics", () => {
    const a = liveRunAdapter(unwiredCrews, run, detail);
    const steps = a.steps("r1");
    expect(steps.state).toBe("live");
    if (steps.state !== "live") return;
    expect(steps.value.map((s) => [s.label, s.startPct, s.endPct, s.status, s.took])).toEqual([
      ["researcher: task", 0, 50, "ok", "20 s"],
      ["analyst: task", 50, 100, "failed", "20 s"],
      ["tool: kb_query", 100, 100, "skipped", "skipped"],
    ]);
    expect(a.handoffs("r1")).toEqual({ state: "live", value: [{ from: "researcher", to: "analyst" }] });
    expect(a.runMetrics("r1")).toEqual({ state: "live", value: { durationS: 48, tokens: 3210, requests: 4, costUsd: 0 } });
  });

  it("is unwired when nothing is loaded, empty, or for another run", () => {
    expect(liveRunAdapter(unwiredCrews, run, undefined).steps("r1")).toEqual({ state: "unwired" });
    expect(liveRunAdapter(unwiredCrews, run, { steps: [], handoffs: [] }).handoffs("r1")).toEqual({ state: "unwired" });
    expect(liveRunAdapter(unwiredCrews, run, detail).steps("other")).toEqual({ state: "unwired" });
  });

  it("never invents metrics: missing totals, unfinished runs and unknown cost are unwired", () => {
    expect(mapMetrics({ ...run, tokens: null })).toBeNull();
    expect(mapMetrics({ ...run, finished_at: null })).toBeNull();
    expect(mapMetrics({ ...run, cost_usd: null })).toBeNull();
    expect(mapMetrics({ ...run, tokens: undefined })).toBeNull();
    expect(liveRunAdapter(unwiredCrews, { ...run, tokens: null }, detail).runMetrics("r1")).toEqual({ state: "unwired" });
  });

  it("leaves an injected or sample adapter alone", () => {
    expect(liveRunAdapter(sampleCrews, run, detail)).toBe(sampleCrews);
  });

  it("mapSteps handles a zero-length run", () => {
    expect(mapSteps([{ id: 1, agent: "a", kind: "task", label: "a: task", start_ms: 0, end_ms: 0, status: "ok" }])[0]).toMatchObject({ startPct: 0, endPct: 0 });
  });

  it("loadRunDetail caches the fetch and keeps the cache when the fetch fails", async () => {
    clearRunDetails();
    await loadRunDetail("r2", vi.fn().mockResolvedValue(detail));
    await loadRunDetail("r2", vi.fn().mockRejectedValue(new Error("down")));
    expect(getRunDetail("r2")).toEqual(detail);
    await loadRunDetail("r4", vi.fn().mockRejectedValue(new Error("down")));
    expect(getRunDetail("r4")).toBeUndefined();
  });

  it("does not start a second fetch while one is pending", async () => {
    clearRunDetails();
    let release!: (d: CrewRunSteps) => void;
    const fetcher = vi.fn()
      .mockImplementationOnce(() => new Promise<CrewRunSteps>((r) => { release = r; }))
      .mockResolvedValue(detail);
    const first = loadRunDetail("r5", fetcher);
    await loadRunDetail("r5", fetcher);
    expect(fetcher).toHaveBeenCalledTimes(1);
    release(detail);
    await first;
    await loadRunDetail("r5", fetcher);
    expect(fetcher).toHaveBeenCalledTimes(2);
  });
});
