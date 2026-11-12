import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { MuseTelemetry } from "../neuro/telemetryStore";
import { INACTIVE_TRAINING, type TrainingState } from "../neuro/training";
import type { useTraining } from "../neuro/useTraining";
import type { SystemSnapshot } from "../types";
import { NeuroField, mmss, rssiText, type NeuroFieldProps } from "./NeuroScreen";

const NOW = new Date("2026-10-28T14:00:00+09:00").getTime();
const live = vi.hoisted(() => ({ telemetry: null as unknown as MuseTelemetry }));
vi.mock("../neuro/useMuseTelemetry", () => ({ useMuseTelemetry: () => live.telemetry }));
vi.mock("../shell/useNow", () => ({ useNow: () => new Date(NOW) }));

const idleTelemetry = (): MuseTelemetry => ({ channels: { TP9: [], AF7: [], AF8: [], TP10: [] }, bands: { delta: 0, theta: 0, alpha: 0, beta: 0, gamma: 0 }, packetCount: 0, sampleCount: 0, lastPacketAt: null, channelLastPacketAt: {} });
const wave = Array.from({ length: 300 }, (_, i) => Math.sin(i / 4) * 20);
const flowing = (patch: Partial<MuseTelemetry> = {}): MuseTelemetry => ({
  channels: { TP9: wave, AF7: wave, AF8: wave, TP10: wave },
  bands: { delta: 31.6, theta: 18.2, alpha: 27.4, beta: 15.1, gamma: 7.7 },
  packetCount: 120, sampleCount: 1200, lastPacketAt: NOW - 1000, channelLastPacketAt: {}, ...patch,
});

function snap(extra: Partial<SystemSnapshot> = {}): SystemSnapshot {
  return { ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra };
}
function trainingWith(state: Partial<TrainingState> = {}, protocolId = "off") {
  return {
    protocolId, setProtocolId: vi.fn(), protocols: [{ id: "alpha", name: "Alpha up" }, { id: "smr", name: "SMR" }],
    state: { ...INACTIVE_TRAINING, ...state }, begin: vi.fn(), summary: vi.fn(), finish: vi.fn(), advance: vi.fn(), finished: null, dismissFinished: vi.fn(),
  } as unknown as ReturnType<typeof useTraining>;
}
function renderNeuro(props: Partial<NeuroFieldProps> = {}) {
  const handlers = { onScan: vi.fn(), onConnect: vi.fn(), onDisconnect: vi.fn(), onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() };
  render(<NeuroField data={snap()} devices={[]} operation={null} error={null} sessionStartedAt={null} sessionEndedAt={null} training={trainingWith()} {...handlers} {...props} />);
  return handlers;
}
const connected = { connected: true, device_name: "Muse S", streaming: false };
const streaming = { connected: true, device_name: "Muse S", streaming: true };

// museSignalState compares against Date.now(); pin it to the same clock as useNow.
beforeEach(() => { vi.useFakeTimers({ toFake: ["Date"] }); vi.setSystemTime(NOW); live.telemetry = idleTelemetry(); });
afterEach(() => { vi.useRealTimers(); });

describe("Neuro screen", () => {
  it("loading: skeleton cards with the heading and no dashes", () => {
    renderNeuro({ data: INITIAL_SNAPSHOT });
    const page = screen.getByLabelText("Loading Neuro");
    expect(page).toHaveAttribute("aria-busy", "true");
    expect(screen.getByRole("heading", { level: 1, name: "Neuro" })).toBeInTheDocument();
    expect(page.textContent).not.toContain("—");
  });

  it("empty: not connected shows Find Muse, pairing guidance and nearby headsets", () => {
    const h = renderNeuro({ devices: [{ name: "Muse-4F21", address: "aa", rssi: -60 }, { name: "Muse-77C0", address: "bb", rssi: -90 }] });
    expect(screen.getByText("Muse · Disconnected")).toBeInTheDocument();
    expect(screen.getByText("No Muse connected")).toBeInTheDocument();
    expect(screen.getByText("Put your headset in pairing mode")).toBeInTheDocument();
    const nearby = screen.getByRole("list", { name: "Nearby headsets" });
    expect(within(nearby).getByText("Strong signal")).toBeInTheDocument();
    expect(within(nearby).getByText("Weak signal")).toBeInTheDocument();
    fireEvent.click(within(nearby).getAllByRole("button", { name: "Connect" })[0]);
    expect(h.onConnect).toHaveBeenCalledWith(expect.objectContaining({ address: "aa" }));
    fireEvent.click(screen.getByRole("button", { name: "Find Muse" }));
    expect(h.onScan).toHaveBeenCalled();
    expect(screen.getByText("Your first completed session will appear here.")).toBeInTheDocument();
  });

  it("loading operation: the primary button shows progress and other controls are disabled", () => {
    renderNeuro({ operation: "scanning" });
    expect(screen.getByRole("button", { name: /Searching…/ })).toBeDisabled();
    expect(screen.getByText("Searching for Muse")).toBeInTheDocument();
  });

  it("connected but idle: Start session + Disconnect, hero tiles Ready, bands at 0 with —", () => {
    const h = renderNeuro({ data: snap({ muse: connected, noop: { ...INITIAL_SNAPSHOT.noop, today: { ...(INITIAL_SNAPSHOT.noop.today ?? {}), recovery: 74 } as SystemSnapshot["noop"]["today"] } }) });
    expect(screen.getByText("Muse S · Connected")).toBeInTheDocument();
    expect(screen.getByText("Recovery 74%")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start session" }));
    expect(h.onStart).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(h.onDisconnect).toHaveBeenCalled();
    expect(screen.getAllByText("Ready")).toHaveLength(3);
    expect(screen.getByRole("progressbar", { name: "Alpha relative power" })).toHaveAttribute("aria-valuenow", "0");
    const select = screen.getByRole("combobox", { name: "Protocol" });
    expect(select).toBeEnabled();
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["Recording only", "Alpha up", "SMR"]);
  });

  it("error: an unreadable Muse bridge is a screen-level alert with Try Again", () => {
    const h = renderNeuro({ data: snap({ unavailableSources: [{ command: "get_muse_state", label: "Muse" }] }) });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Muse status couldn't be read");
    fireEvent.click(within(alert).getByRole("button", { name: "Try Again" }));
    expect(h.onRetry).toHaveBeenCalled();
  });

  it("error: a Muse operation error during a session offers Retry Stop and Save", () => {
    const h = renderNeuro({ data: snap({ muse: streaming }), error: "Couldn't stop the EEG stream", sessionStartedAt: NOW - 65_000 });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Couldn't stop the EEG stream");
    fireEvent.click(within(alert).getByRole("button", { name: "Retry Stop and Save" }));
    expect(h.onStop).toHaveBeenCalled();
  });

  it("save pending: Retry Save is the primary action and a warning explains it", () => {
    renderNeuro({ data: snap({ muse: connected }), sessionStartedAt: NOW - 125_000, sessionEndedAt: NOW - 5_000 });
    expect(screen.getByRole("button", { name: "Retry Save" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Recording stopped at 02:00. Saving is pending; keep Amara OS open.");
  });

  it("streaming: Stop session (destructive), elapsed time, live bands, signal trace and diagnostics", () => {
    live.telemetry = flowing();
    const h = renderNeuro({ data: snap({ muse: streaming }), sessionStartedAt: NOW - 65_000 });
    expect(screen.getByText("Muse S · Streaming")).toBeInTheDocument();
    const stop = screen.getByRole("button", { name: "Stop session" });
    expect(stop).toHaveAttribute("data-variant", "destructive");
    fireEvent.click(stop);
    expect(h.onStop).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Disconnect" })).toBeNull();
    expect(document.querySelector(".neuro__session-time .ui-stat__value")).toHaveTextContent("01:05");
    expect(screen.getByText("27%")).toBeInTheDocument(); // alpha 27.4
    expect(screen.getByRole("progressbar", { name: "Alpha relative power" })).toHaveAttribute("aria-valuenow", "27.4");
    expect(screen.getByText("Relative power · 1–45 Hz · not a medical measurement.")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /TP9 raw EEG, last 10 seconds/ })).toBeInTheDocument();
    expect(screen.getByText("1s ago")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Protocol" })).toBeDisabled();
  });

  it("streaming with fewer than 256 samples shows Acquiring…", () => {
    live.telemetry = flowing({ sampleCount: 120 });
    renderNeuro({ data: snap({ muse: streaming }), sessionStartedAt: NOW - 2_000 });
    expect(screen.getAllByText("Acquiring…")).toHaveLength(5);
  });

  it("signal interrupted (>2.5 s since a packet) shows a red badge", () => {
    live.telemetry = flowing({ lastPacketAt: NOW - 4_000 });
    renderNeuro({ data: snap({ muse: streaming }), sessionStartedAt: NOW - 30_000 });
    expect(screen.getAllByText(/Signal interrupted/).length).toBeGreaterThan(0);
    const badge = screen.getByText("Signal interrupted", { selector: ".ui-badge" });
    expect(badge).toHaveAttribute("data-tone", "danger");
  });

  it("training: calibration meter, held badge with label, Calm ring (Beta)", () => {
    live.telemetry = flowing();
    const { unmount } = render(<NeuroField data={snap({ muse: streaming })} devices={[]} operation={null} error={null} sessionStartedAt={NOW - 3_000} sessionEndedAt={null}
      training={trainingWith({ phase: "calibrating", label: "Calibrating", calibrationPercent: 40 }, "alpha")}
      onScan={vi.fn()} onConnect={vi.fn()} onDisconnect={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);
    expect(screen.getByRole("progressbar", { name: "Calibration" })).toHaveAttribute("aria-valuenow", "40");
    unmount();
    renderNeuro({ data: snap({ muse: streaming }), sessionStartedAt: NOW - 90_000, training: trainingWith({ phase: "held", label: "Movement detected", calibrationPercent: 100, calmPercent: 62.4, focus: null, relaxation: null }, "alpha") });
    expect(screen.getByText("Movement detected")).toHaveAttribute("data-tone", "warning");
    expect(screen.getByRole("meter", { name: "Calm" })).toHaveAttribute("aria-valuenow", "62");
    expect(screen.getByText("62%")).toBeInTheDocument();
    expect(screen.getAllByText("Beta").length).toBeGreaterThan(0);
  });

  it("recent sessions: up to 5 rows; selecting one opens its summary, old sessions show — for Calm", () => {
    const session = (id: string, summary: SystemSnapshot["sessions"][number]["summary"] = {}) => ({ id, device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:20:00+09:00", duration_secs: 1200, data_points: 9000, summary });
    const sessions = [session("new", { calm_pct: 61.6, time_in_target_pct: 40, avg_focus: 52, avg_relaxation: 58 }), session("old", { avg_relaxation: 44 }), ...["a", "b", "c", "d"].map((id) => session(id))];
    renderNeuro({ data: snap({ muse: connected, sessions }) });
    const recent = screen.getByRole("list", { name: "Recent sessions" });
    expect(within(recent).getAllByRole("listitem")).toHaveLength(5);
    const rows = within(recent).getAllByRole("button");
    fireEvent.click(rows[0]);
    let panel = screen.getByRole("complementary");
    expect(panel).toHaveTextContent("Session summary");
    expect(panel).toHaveTextContent("Calm Beta62%");
    expect(panel).toHaveTextContent("In target40%");
    fireEvent.click(rows[1]);
    panel = screen.getByRole("complementary");
    expect(panel).toHaveTextContent("Calm Beta—");
    fireEvent.click(within(panel).getByRole("button", { name: "Close session summary" }));
    expect(screen.queryByRole("complementary")).toBeNull();
  });
});

describe("Neuro sections (1112n)", () => {
  it("shows the Hemi-Sync and trend sections in the Mind–Body page language, with honest unwired states", () => {
    renderNeuro({ data: snap({ muse: connected }) });
    expect(screen.getByRole("heading", { level: 2, name: "Hemi-Sync" })).toBeInTheDocument();
    expect(screen.getByText("Your Hemi-Sync library isn't connected yet.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "Done with the Muse" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "Focus and calm over time" })).toBeInTheDocument();
    expect(screen.getByText("Trends appear after two sessions with a focus or calm score.")).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
  });

  it("trends: two or more scored sessions draw Focus and Calm rows from the real list", () => {
    const at = (day: number, focus: number, calm: number) => ({ id: `s${day}`, device_type: "muse", start: `2026-10-${day}T21:00:00+09:00`, end: `2026-10-${day}T21:20:00+09:00`, duration_secs: 1200, data_points: 9000, summary: { avg_focus: focus, avg_relaxation: calm } });
    renderNeuro({ data: snap({ muse: connected, sessions: [at(20, 50, 60), at(22, 58, 64), at(24, 62, 70)] }) });
    expect(screen.getByText("Focus averaged 57 over your last 3 sessions.")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Focus across your last 3 sessions, latest 62/ })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Calm across your last 3 sessions, latest 70/ })).toBeInTheDocument();
  });

  it("disconnected: the Hemi-Sync section still shows", () => {
    renderNeuro();
    expect(screen.getByRole("heading", { level: 2, name: "Hemi-Sync" })).toBeInTheDocument();
  });
});

describe("helpers", () => {
  it("formats elapsed time and signal strength", () => {
    expect(mmss(65)).toBe("01:05");
    expect(mmss(-3)).toBe("00:00");
    expect(rssiText(-64)).toBe("Strong signal");
    expect(rssiText(-79)).toBe("Fair signal");
    expect(rssiText(-80)).toBe("Weak signal");
    expect(rssiText(null)).toBe("Signal strength unavailable");
  });
});
