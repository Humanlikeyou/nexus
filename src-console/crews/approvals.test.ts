import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { decideCrewApproval, listCrewApprovals, startCrew } from "../bridge";
import type { CrewApprovalRow } from "../types";
import { sampleCrews, unwiredCrews } from "./adapters";
import { ago, getApprovals, isCrewApproval, liveApprovalsAdapter, loadApprovals, mapDecided, mapPending, setApprovals } from "./approvals";

const row = (over: Partial<CrewApprovalRow> = {}): CrewApprovalRow => ({
  id: "a1", run_id: "r1", crew: "Research", agents: ["researcher", "writer"], kind: "write", items: ["edit_file", "send_note"],
  title: "Allow this run to make changes", reason: "the run wants to change something", risk: "Medium", requested_at: "2026-01-01T00:00:00Z", ...over,
});
const NOW = Date.parse("2026-01-01T00:10:00Z");

describe("crew approvals mapping", () => {
  it("maps a pending request to the adapter shape; amount is never wired", () => {
    const [a] = mapPending([row()], NOW);
    expect(a).toEqual({ id: "a1", title: "Allow this run to make changes", crew: "Research", agents: "researcher → writer", impact: "Changes · edit_file, send_note", requested: "10 min ago", risk: "Medium", reason: "the run wants to change something", amount: { state: "unwired" } });
  });

  it("maps decided rows with approved only when explicitly true", () => {
    const rows = [row({ id: "d1", approved: true, decided_at: "2026-01-01T00:05:00Z" }), row({ id: "d2", approved: false, decided_at: "2026-01-01T00:06:00Z" }), row({ id: "d3", approved: null })];
    expect(mapDecided(rows).map((d) => [d.id, d.approved])).toEqual([["d1", true], ["d2", false], ["d3", false]]);
  });

  it("an unknown risk shows as Medium, never invented as Low", () => {
    expect(mapPending([row({ risk: "Zero" })], NOW)[0].risk).toBe("Medium");
  });

  it("formats relative times", () => {
    expect(ago("2026-01-01T00:00:00Z", Date.parse("2026-01-01T00:00:01Z"))).toBe("just now");
    expect(ago("2026-01-01T00:00:00Z", Date.parse("2026-01-01T01:00:00Z"))).toBe("1 h ago");
    expect(ago("nope", NOW)).toBe("");
  });
});

describe("crew approvals are a separate queue from Terrance's", () => {
  it("drops anything that is not a crew-service approval (e.g. Terrance ledger rows)", () => {
    const terrance = { id: "t1", title: "Send reply", status: "pending", kind: "message", source: "terrance" };
    const noRun = { ...row(), run_id: undefined };
    expect(isCrewApproval(terrance)).toBe(false);
    expect(isCrewApproval(noRun)).toBe(false);
    expect(isCrewApproval(null)).toBe(false);
    expect(mapPending([terrance as unknown as CrewApprovalRow, row()], NOW).map((a) => a.id)).toEqual(["a1"]);
    expect(mapDecided([terrance as unknown as CrewApprovalRow]).length).toBe(0);
  });
});

describe("live approvals adapter", () => {
  beforeEach(() => { setApprovals(null); invoke.mockReset(); });

  it("stays unwired when the service is down or not loaded", () => {
    expect(liveApprovalsAdapter(unwiredCrews, null).approvals()).toEqual({ state: "unwired" });
    expect(liveApprovalsAdapter(unwiredCrews, null).decided()).toEqual({ state: "unwired" });
  });

  it("is live over the unwired adapter and leaves sample mode alone", () => {
    const data = { pending: [row()], decided: [row({ id: "d1", approved: true, decided_at: "2026-01-01T00:05:00Z" })] };
    const live = liveApprovalsAdapter(unwiredCrews, data, NOW);
    expect(live.approvals().state).toBe("live");
    expect(live.decided().state).toBe("live");
    expect(liveApprovalsAdapter(sampleCrews, data, NOW)).toBe(sampleCrews);
    // other panels are untouched
    expect(live.steps("r1")).toEqual({ state: "unwired" });
  });

  it("loadApprovals caches both lists, and a failure clears the cache to unwired", async () => {
    const fetcher = vi.fn(async (s: "pending" | "decided") => (s === "pending" ? [row()] : []));
    await loadApprovals(fetcher);
    expect(getApprovals()?.pending.length).toBe(1);
    await loadApprovals(async () => { throw new Error("down"); });
    expect(getApprovals()).toBeNull();
    await loadApprovals(async () => undefined as never); // a malformed reply is not "no approvals"
    expect(getApprovals()).toBeNull();
  });
});

describe("bridge wrappers", () => {
  beforeEach(() => invoke.mockReset());
  it("calls the crew commands with the right arguments", async () => {
    invoke.mockResolvedValue([]);
    await listCrewApprovals("pending");
    await decideCrewApproval("a1", true);
    await startCrew("n", "t");
    await startCrew("n", "t", "r1");
    expect(invoke.mock.calls).toEqual([
      ["crew_list_approvals", { state: "pending" }],
      ["crew_decide_approval", { id: "a1", approve: true }],
      ["crew_start", { name: "n", task: "t", agents: null, retryOf: null }],
      ["crew_start", { name: "n", task: "t", agents: null, retryOf: "r1" }],
    ]);
  });
});
