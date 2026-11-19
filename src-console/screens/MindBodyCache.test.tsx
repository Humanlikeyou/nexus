import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockSeriesPayload, mockSnapshot } from "../lib/mindBody";
import { readSnapshot, resetSharedSnapshot } from "../lib/noopSnapshot";
import { MindBodyField, clearMindBodyCache, loadMindBody, seriesFromSnapshot, type MindBodyState } from "./MindBodyScreen";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

type Call = { tool: string; args: { key?: string } | null };
const batchBackend = (snapshot = mockSnapshot()) => async (cmd: string, payload: { calls?: Call[]; tool?: string; args?: { key?: string } }) => {
  if (cmd === "noop_query_batch") return (payload.calls ?? []).map((c) => ({ value: c.tool === "health_snapshot" ? snapshot : mockSeriesPayload(c.args?.key as never, [60, 61]), error: null }));
  if (cmd === "noop_query") return payload.tool === "health_snapshot" ? snapshot : mockSeriesPayload(payload.args?.key as never, [60, 61]);
  throw new Error(`unexpected ${cmd}`);
};
const noopCalls = (cmd: string) => invokeMock.mock.calls.filter((c) => c[0] === cmd);

beforeEach(() => { invokeMock.mockReset(); clearMindBodyCache(); resetSharedSnapshot(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("loadMindBody reads", () => {
  it("asks for all six reads in one batch call, with no separate single reads", async () => {
    invokeMock.mockImplementation(batchBackend());
    expect((await loadMindBody()).status).toBe("ready");
    expect(noopCalls("noop_query")).toHaveLength(0);
    const batches = noopCalls("noop_query_batch");
    expect(batches).toHaveLength(1);
    const calls = (batches[0][1] as { calls: Call[] }).calls;
    expect(calls.map((c) => c.tool === "health_snapshot" ? "snap" : c.args?.key)).toEqual(["snap", "sleep_total_min", "hrv", "recovery", "rhr", "strain"]);
    expect(calls[0].args).toEqual({ days: 29 });
    expect(calls[1].args).toEqual({ key: "sleep_total_min", days: 365 });
  });
  it("a session error fails the page the same way a single read did", async () => {
    invokeMock.mockRejectedValue({ kind: "timeout", detail: 10 });
    expect((await loadMindBody()).status).toBe("error");
  });
  it("optional rhr/strain failing in the batch still gives a ready page", async () => {
    invokeMock.mockImplementation(async (cmd: string, p: { calls: Call[] }) => cmd === "noop_query_batch"
      ? p.calls.map((c) => c.args?.key === "rhr" || c.args?.key === "strain" ? { value: null, error: { kind: "tool_error", detail: "x" } } : { value: c.tool === "health_snapshot" ? mockSnapshot() : mockSeriesPayload(c.args?.key as never, [60]), error: null })
      : undefined);
    const s = await loadMindBody();
    expect(s.status).toBe("ready");
    if (s.status === "ready") expect(s.long?.resting_hr).toEqual([]);
  });
  it("falls back to parallel single reads when the batch command is unavailable", async () => {
    invokeMock.mockImplementation(async (cmd: string, p: { tool?: string; args?: { key?: string } }) => {
      if (cmd === "noop_query_batch") throw new Error("unknown command");
      return p.tool === "health_snapshot" ? mockSnapshot() : mockSeriesPayload(p.args?.key as never, [60]);
    });
    expect((await loadMindBody()).status).toBe("ready");
    expect(noopCalls("noop_query")).toHaveLength(6);
  });
});

describe("shared snapshot", () => {
  it("the take reads the snapshot the page already fetched: one health_snapshot read in total", async () => {
    invokeMock.mockImplementation(batchBackend());
    const page = loadMindBody();
    const take = readSnapshot(); // asked while the page's read is still in flight
    expect((await take)?.generatedAt).toBe(mockSnapshot().generatedAt);
    await page;
    expect((await readSnapshot())?.generatedAt).toBe(mockSnapshot().generatedAt);
    expect(noopCalls("noop_query_batch")).toHaveLength(1);
    expect(noopCalls("noop_query")).toHaveLength(0);
  });
  it("without a page read it fetches once for concurrent askers, and never keeps a failure", async () => {
    invokeMock.mockImplementation(batchBackend());
    await Promise.all([readSnapshot(), readSnapshot()]);
    expect(noopCalls("noop_query")).toHaveLength(1);
    resetSharedSnapshot(); invokeMock.mockReset();
    invokeMock.mockRejectedValue({ kind: "timeout", detail: 10 });
    expect(await readSnapshot()).toBeNull();
    invokeMock.mockImplementation(batchBackend());
    expect(await readSnapshot()).not.toBeNull();
  });
});

const readyState = (): MindBodyState => ({ status: "ready", series: seriesFromSnapshot(mockSnapshot()), drivers: { sleep: [], hrv: [], recovery: [] } });
const heading = () => screen.queryByRole("heading", { name: "Readiness" });
const skeleton = () => screen.queryAllByRole("status").some((e) => /loading/i.test(e.getAttribute("aria-label") ?? ""));

describe("Mind-Body stale-while-revalidate", () => {
  it("second visit renders cached content at once, refreshes in the background, then updates", async () => {
    const load = vi.fn(async () => readyState());
    const first = render(<MindBodyField sessions={[]} load={load} />);
    expect(skeleton()).toBe(true);
    await screen.findByRole("heading", { name: "Readiness" });
    first.unmount();
    let release!: (s: MindBodyState) => void;
    load.mockImplementation(() => new Promise<MindBodyState>((r) => { release = r; }));
    render(<MindBodyField sessions={[]} load={load} />);
    // No skeleton and the content is there synchronously, before the refresh resolves.
    expect(skeleton()).toBe(false);
    expect(heading()).toBeInTheDocument();
    expect(load).toHaveBeenCalledTimes(2); // the refresh was started
    expect(screen.queryByText("Collecting data: 0/15 sessions")).toBeInTheDocument();
    // New data arrives: the page updates (an empty-history result swaps in the setup state).
    release({ status: "inactive", steps: "Fresh result replaced the cached one." });
    expect(await screen.findByText("Fresh result replaced the cached one.")).toBeInTheDocument();
  });
  it("a failed refresh keeps showing the cached page; a setup problem replaces it", async () => {
    const load = vi.fn(async () => readyState());
    render(<MindBodyField sessions={[]} load={load} />);
    await screen.findByRole("heading", { name: "Readiness" });
    cleanup();
    load.mockResolvedValue({ status: "error" });
    render(<MindBodyField sessions={[]} load={load} />);
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    await Promise.resolve();
    expect(heading()).toBeInTheDocument();
    expect(screen.queryByText("Couldn't load this chart.")).not.toBeInTheDocument();
    cleanup();
    load.mockResolvedValue({ status: "inactive", steps: null });
    render(<MindBodyField sessions={[]} load={load} />);
    expect(await screen.findByRole("region", { name: "Health sync setup" })).toBeInTheDocument();
  });
  it("a different loader never gets another loader's cached page", async () => {
    render(<MindBodyField sessions={[]} load={async () => readyState()} />);
    await screen.findByRole("heading", { name: "Readiness" });
    cleanup();
    render(<MindBodyField sessions={[]} load={() => new Promise<MindBodyState>(() => {})} />);
    expect(skeleton()).toBe(true);
  });
  it("a new Tokyo day never shows yesterday's cached page", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-11-19T03:00:00Z"));
    const load = vi.fn(async () => readyState());
    render(<MindBodyField sessions={[]} load={load} />);
    await screen.findByRole("heading", { name: "Readiness" });
    cleanup();
    // Same Tokyo day later: cached.
    vi.setSystemTime(new Date("2026-11-19T10:00:00Z"));
    render(<MindBodyField sessions={[]} load={load} />);
    expect(skeleton()).toBe(false);
    await screen.findByRole("heading", { name: "Readiness" });
    cleanup();
    // 16:00Z is already the 20th in Tokyo: skeleton again.
    vi.setSystemTime(new Date("2026-11-19T16:00:00Z"));
    load.mockImplementation(() => new Promise<MindBodyState>(() => {}));
    render(<MindBodyField sessions={[]} load={load} />);
    expect(skeleton()).toBe(true);
  });
});
