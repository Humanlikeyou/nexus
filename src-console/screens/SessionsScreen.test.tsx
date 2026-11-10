import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { RecordedSession, SystemSnapshot } from "../types";
import { SessionsField } from "./SessionsScreen";

const session = (id: string, o: Partial<RecordedSession> = {}): RecordedSession => ({
  id, device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:12:00+09:00", duration_secs: 720, data_points: 46080,
  summary: { avg_focus: 61, avg_relaxation: 48 }, ...o,
});
const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });

describe("Sessions (Neuro › History)", () => {
  it("shows tiles, the list and the newest session's detail", () => {
    render(<SessionsField data={snap({ sessions: [session("a", { summary: { avg_focus: 61, avg_relaxation: 48, calm_pct: 42.4, time_in_target_pct: null } }), session("b", { device_type: "polar", duration_secs: 20 })] })} />);
    expect(screen.getByText(/^\d+ sessions?, \d+ min$/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Neuro history" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "History" })).toBeInTheDocument();
    const detail = screen.getByRole("region", { name: "Muse session" });
    expect(within(detail).getByText("Saved locally")).toBeInTheDocument();
    expect(within(detail).getByText("42")).toBeInTheDocument();
    expect(screen.getByText(/aren.t clinical measurements/)).toBeInTheDocument();
  });

  it("selects with a click and with ↓/↑", () => {
    render(<SessionsField data={snap({ sessions: [session("a"), session("b", { device_type: "polar" })] })} />);
    fireEvent.click(screen.getByRole("button", { name: /polar session/ }));
    expect(screen.getByRole("region", { name: "polar session" })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("button", { name: /polar session/ }), { key: "ArrowUp" });
    expect(screen.getByRole("region", { name: "Muse session" })).toBeInTheDocument();
  });

  it("empty state offers Start session; error state offers Retry", () => {
    const onStart = vi.fn();
    const { unmount } = render(<SessionsField data={snap({ sessions: [] })} onStartSession={onStart} />);
    expect(screen.getByRole("heading", { level: 1, name: "Neuro history" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start session" }));
    expect(onStart).toHaveBeenCalledTimes(1);
    unmount();
    const onRetry = vi.fn();
    render(<SessionsField data={snap({ sessions: [], unavailableSources: [{ command: "list_sessions", label: "Sensor sessions" }] })} onRetry={onRetry} />);
    expect(screen.getByRole("heading", { level: 1, name: "Neuro history" })).toBeInTheDocument();
    expect(screen.getByText("Sensor sessions unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});

describe("design-1110c Neuro history", () => {
  it("a real session without samples says graphs aren't recorded yet, and no sample data shows", () => {
    render(<SessionsField data={snap({ sessions: [session("a")] })} samples={false} />);
    expect(screen.getByText("Graphs over time aren't recorded yet")).toBeInTheDocument();
    expect(screen.queryByText(/Sample data/)).toBeNull();
    expect(screen.queryByRole("region", { name: "Sample sessions" })).toBeNull();
  });

  it("sample mode adds a separate, labelled sample section and never touches the real list", () => {
    render(<SessionsField data={snap({ sessions: [session("a"), session("b", { start: "2026-10-28T21:00:00+09:00", summary: { avg_focus: 70, avg_relaxation: 52 } })] })} samples />);
    expect(within(screen.getByRole("list", { name: "History" })).getAllByRole("button")).toHaveLength(2);
    const sample = screen.getByRole("region", { name: "Sample sessions" });
    expect(within(sample).getAllByText("Sample").length).toBe(8);
    expect(within(sample).getAllByText("Sample data").length).toBeGreaterThanOrEqual(3);
    expect(within(sample).getByRole("radiogroup", { name: "Session view" })).toBeInTheDocument();
    expect(within(sample).getAllByRole("img", { name: /band power over the session/ })).toHaveLength(5);
    fireEvent.click(within(sample).getByRole("radio", { name: "Focus & calm" }));
    expect(within(sample).getByRole("img", { name: /^Focus over the session/ })).toBeInTheDocument();
    fireEvent.click(within(sample).getByRole("radio", { name: "Sensors" }));
    expect(within(sample).getByRole("img", { name: /Muse sensors: TP9/ })).toBeInTheDocument();
  });
});
