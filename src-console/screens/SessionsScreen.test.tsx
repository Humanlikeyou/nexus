import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { RecordedSession, SystemSnapshot } from "../types";
import { createSampleRecordings, liveRecordings } from "../neuro/recordings";
import { SessionsField } from "./SessionsScreen";

const session = (id: string, o: Partial<RecordedSession> = {}): RecordedSession => ({
  id, device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:12:00+09:00", duration_secs: 720, data_points: 46080, file_path: `/x/eeg_${id}.ndjson`,
  summary: { avg_focus: 61, avg_relaxation: 48 }, ...o,
});
const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const badge = (text: string) => (_: string, el: Element | null) => !!el?.classList.contains("rec-badge") && el.textContent === text;
const NOW = Date.parse("2026-11-10T18:00:00+09:00");

describe("Neuro history (design-1110e recordings)", () => {
  it("lists real sessions as 4-channel recordings, with no sample tag and honest Needs wiring states", () => {
    const sessions = [session("a"), session("b", { start: "2026-11-07T11:57:00+09:00", duration_secs: 682 })];
    render(<SessionsField data={snap({ sessions })} adapter={liveRecordings(sessions, NOW)} />);
    expect(screen.getByRole("heading", { level: 1, name: "Neuro history" })).toBeInTheDocument();
    expect(screen.getByText("2 recordings, 23 min")).toBeInTheDocument();
    const table = screen.getByRole("table", { name: "Recordings" });
    expect(within(table).getAllByText(badge("4 channels"))).toHaveLength(2);
    expect(screen.queryByText("Sample data")).toBeNull();
    fireEvent.click(within(table).getAllByRole("button")[0]);
    expect(screen.getByRole("heading", { level: 1, name: "Recording" })).toBeInTheDocument();
    expect(screen.getByText(/saved on this Mac, but Nexus can't read it back yet/)).toBeInTheDocument();
    expect(screen.getByText(/Heart \(PPG\) and motion aren.t recorded yet/)).toBeInTheDocument();
    expect(screen.queryByText(/aren.t recorded yet$/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "‹ Neuro history" }));
    expect(screen.getByRole("table", { name: "Recordings" })).toBeInTheDocument();
  });

  it("marks recordings older than 90 days as needing his OK, and never deletes them on its own", () => {
    const sessions = [session("old", { start: "2026-02-18T22:51:00+09:00" }), session("new", { start: "2026-11-07T11:57:00+09:00" })];
    render(<SessionsField data={snap({ sessions })} adapter={liveRecordings(sessions, NOW)} />);
    expect(screen.getByText(/Over 90 days · needs your OK/)).toBeInTheDocument();
    expect(screen.getByText(/1 recording is older than 90 days/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Review 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove 1 recording" }));
    expect(screen.getByRole("status")).toHaveTextContent("Removing recordings needs wiring, so nothing was deleted.");
  });

  it("E12: no removal happens without the dialog's confirmation, and only what was ticked", () => {
    const adapter = createSampleRecordings();
    const spy = vi.spyOn(adapter, "removeConfirmed");
    render(<SessionsField data={snap({ sessions: [] })} adapter={adapter} />);
    expect(screen.getAllByText(/Over 90 days · needs your OK/)).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: "Keep them" }));
    expect(spy).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /^Review/ })).toBeNull();
    expect(adapter.removals).toHaveLength(0);
  });

  it("E12: Review then Keep all removes nothing; Remove passes only the ticked ids with a confirmation", () => {
    const adapter = createSampleRecordings();
    render(<SessionsField data={snap({ sessions: [] })} adapter={adapter} />);
    fireEvent.click(screen.getByRole("button", { name: "Review 3" }));
    fireEvent.click(screen.getByRole("button", { name: "Keep all" }));
    expect(adapter.removals).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Review 3" }));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getByRole("button", { name: "Remove 2 recordings" }));
    expect(adapter.removals).toHaveLength(1);
    expect(adapter.removals[0].ids).toEqual(["r8", "r9"]);
    expect(adapter.removals[0].confirmation.confirmedByUser).toBe(true);
    expect(screen.getByRole("status")).toHaveTextContent("Sample data: nothing was deleted (2 would be removed).");
  });

  it("sample mode labels everything and opens a 5-channel recording with AUX occipital, heart and motion", () => {
    render(<SessionsField data={snap({ sessions: [] })} adapter={createSampleRecordings()} />);
    expect(screen.getAllByText("Sample data").length).toBeGreaterThan(1);
    fireEvent.click(screen.getByRole("button", { name: /Nov 7, 12:09 PM/ }));
    expect(screen.getAllByText(badge("5 channels · AUX")).length).toBeGreaterThan(0);
    expect(screen.getByRole("img", { name: /AUX occipital raw EEG/ })).toBeInTheDocument();
    for (const c of ["TP9", "AF7", "AF8", "TP10"]) expect(screen.getByRole("img", { name: new RegExp(`^${c} raw EEG`) })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Heart, PPG/ })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Relative band power/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "10 s" }));
    expect(screen.getByRole("img", { name: /^TP9 raw EEG, 00:50(\.\d+)? to 01:00/ })).toBeInTheDocument();
  });

  it("empty state offers Start session; error state offers Retry", () => {
    const onStart = vi.fn();
    const { unmount } = render(<SessionsField data={snap({ sessions: [] })} adapter={liveRecordings([])} onStartSession={onStart} />);
    expect(screen.getByText("No recordings yet")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start session" }));
    expect(onStart).toHaveBeenCalledTimes(1);
    unmount();
    const onRetry = vi.fn();
    render(<SessionsField data={snap({ sessions: [], unavailableSources: [{ command: "list_sessions", label: "Sensor sessions" }] })} adapter={liveRecordings([])} onRetry={onRetry} />);
    expect(screen.getByText("Sensor sessions unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
