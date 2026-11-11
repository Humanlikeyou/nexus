import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { mockDayRow, mockSeriesPayload, mockSnapshot } from "../lib/mindBody";
import { DISCLAIMER, INVALID_PATH_STEPS, MISSING_STEPS, MindBodyField, SANDBOX_STEPS, loadMindBody, seriesFromSnapshot, type MindBodyState } from "./MindBodyScreen";

describe("Mind-Body", () => {
  it("shows setup steps when NOOP is missing", async () => {
    const { container } = render(<MindBodyField sessions={[]} load={() => Promise.resolve<MindBodyState>({ status: "inactive", steps: null })} />);
    expect(await screen.findByRole("region", { name: "Health sync setup" })).toBeInTheDocument();
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
  it("reads the latest synced day while today's row is still empty", async () => {
    const snap = mockSnapshot();
    const empty = mockDayRow(snap.logicalToday, { totalSleepMin: null, restingHr: null, avgHrv: null, recovery: null, strain: null });
    const { latestNight: _night, ...rest } = snap;
    const series = seriesFromSnapshot({ ...rest, today: empty });
    render(<MindBodyField sessions={[]} load={() => Promise.resolve<MindBodyState>({ status: "ready", series, drivers: { sleep: [], hrv: [], recovery: [] } })} />);
    expect(await screen.findByLabelText(/^Readiness: (High|Moderate|Low)$/)).toBeInTheDocument();
    expect(screen.getByText(/^Reading for Nov 3, the latest with data/)).toBeInTheDocument();
    expect(screen.getByLabelText(/^Recovery \d+%$/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 3, name: "HRV" }).nextSibling).toHaveTextContent(/^\d+ ms$/);
  });
  it("names no earlier day when today has values", async () => {
    render(<MindBodyField sessions={[]} load={() => Promise.resolve<MindBodyState>({ status: "ready", series: seriesFromSnapshot(mockSnapshot()), drivers: { sleep: [], hrv: [], recovery: [] } })} />);
    await screen.findByRole("heading", { name: "Readiness" });
    expect(screen.queryByText(/latest reading/)).not.toBeInTheDocument();
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
    // design-1111s: fixed copy; the backend detail (which named the legacy helper) is never shown.
    expect(await loadMindBody()).toEqual({ status: "inactive", steps: MISSING_STEPS });
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

  it("says plainly when the latest reading is old, and never scores it", async () => {
    const days = Array.from({ length: 10 }, (_, i) => ({ day: `2026-10-${String(i + 1).padStart(2, "0")}`, value: 60 }));
    const blank = { day: "2026-10-20", value: null };
    const series = { hrv: { key: "hrv", points: [...days, blank] }, recovery: { key: "recovery", points: [...days, blank] }, sleep: { key: "sleep_total_min", points: [...days, blank] }, resting_hr: { key: "rhr", points: [] }, strain: { key: "strain", points: [] } };
    render(<MindBodyField sessions={[]} load={async () => ({ status: "ready", series, drivers: { sleep: [], hrv: [], recovery: [] } } as never)} />);
    expect(await screen.findByText("No recent reading")).toBeInTheDocument();
    expect(screen.getByText("The last reading was Oct 10. Sync from Settings › Devices.")).toBeInTheDocument();
    expect(screen.queryByLabelText(/^Readiness:/)).not.toBeInTheDocument();
  });
});
