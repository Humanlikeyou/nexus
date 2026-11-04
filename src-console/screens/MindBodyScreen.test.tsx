import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { mockSeriesPayload, mockSnapshot } from "../lib/mindBody";
import { DISCLAIMER, INVALID_PATH_STEPS, MindBodyField, SANDBOX_STEPS, loadMindBody, seriesFromSnapshot, type MindBodyState } from "./MindBodyScreen";

describe("Mind-Body", () => {
  it("shows setup steps when NOOP is missing", async () => {
    const { container } = render(<MindBodyField sessions={[]} load={() => Promise.resolve<MindBodyState>({ status: "inactive", steps: null })} />);
    expect(await screen.findByRole("region", { name: "NOOP setup" })).toBeInTheDocument();
    expect(container.querySelector(".subsystem-inactive")).not.toBeNull();
    expect(screen.getByText(DISCLAIMER)).toBeInTheDocument();
  });
  it("shows a retry state for bad output", async () => {
    render(<MindBodyField sessions={[]} load={() => Promise.resolve<MindBodyState>({ status: "error" })} />);
    expect(await screen.findByText("Couldn't load this chart.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
  it("uses binary_missing detail as setup steps", async () => {
    render(<MindBodyField sessions={[]} load={() => Promise.resolve<MindBodyState>({ status: "inactive", steps: "Set the NOOP binary path in Settings." })} />);
    expect(await screen.findByText("Set the NOOP binary path in Settings.")).toBeInTheDocument();
  });
  it("renders three panels with readiness and the collecting counter", async () => {
    render(<MindBodyField sessions={[]} load={() => Promise.resolve<MindBodyState>({ status: "ready", series: seriesFromSnapshot(mockSnapshot()), drivers: { sleep: [], hrv: [], recovery: [] } })} />);
    expect(await screen.findByRole("heading", { name: "Readiness" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Focus drivers" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Session body context" })).toBeInTheDocument();
    expect(screen.getByLabelText(/^Readiness: /)).toBeInTheDocument();
    expect(screen.getByText("Collecting data: 0/15 sessions")).toBeInTheDocument();
    expect(screen.getByText("No sessions yet.")).toBeInTheDocument();
    expect(DISCLAIMER).toBe("Personal estimates, not medical data.");
  });
});

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

describe("loadMindBody", () => {
  it("ready when NOOP omits its nil fields", async () => {
    const { today: _today, latestSleep: _sleep, latestNight: _night, battery: _battery, ...snapshot } = mockSnapshot();
    invokeMock.mockImplementation(async (_cmd: string, { tool, args }: { tool: string; args: { key?: string } }) =>
      tool === "health_snapshot" ? snapshot : mockSeriesPayload(args.key as never, [60]));
    expect((await loadMindBody()).status).toBe("ready");
  });
  it("ready with real shapes, empty driver points allowed", async () => {
    invokeMock.mockImplementation(async (_cmd: string, { tool, args }: { tool: string; args: { key?: string } }) =>
      tool === "health_snapshot" ? mockSnapshot() : mockSeriesPayload(args.key as never, []));
    const state = await loadMindBody();
    expect(state.status).toBe("ready");
    if (state.status === "ready") { expect(state.series.hrv.points).toHaveLength(29); expect(state.drivers.sleep).toEqual([]); }
  });
  it("binary_missing → inactive with detail", async () => {
    invokeMock.mockRejectedValue({ kind: "binary_missing", detail: "Build NOOP first." });
    expect(await loadMindBody()).toEqual({ status: "inactive", steps: "Build NOOP first." });
  });
  it("invalid_path / sandbox_missing → inactive with fixed copy, no raw detail", async () => {
    invokeMock.mockRejectedValue({ kind: "invalid_path", detail: "/Users/x/noop" });
    expect(await loadMindBody()).toEqual({ status: "inactive", steps: INVALID_PATH_STEPS });
    invokeMock.mockRejectedValue({ kind: "sandbox_missing", detail: "/Users/x/Library" });
    expect(await loadMindBody()).toEqual({ status: "inactive", steps: SANDBOX_STEPS });
  });
  it("bad_output / process_failed / bad shape → retry state", async () => {
    invokeMock.mockRejectedValue({ kind: "bad_output", detail: "x" });
    expect((await loadMindBody()).status).toBe("error");
    invokeMock.mockRejectedValue({ kind: "process_failed", detail: "x" });
    expect((await loadMindBody()).status).toBe("error");
    invokeMock.mockReset(); invokeMock.mockResolvedValue({ nope: true });
    expect((await loadMindBody()).status).toBe("error");
  });
});
