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
    expect(screen.getByText("Total time")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "History" })).toBeInTheDocument();
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

  it("empty state offers Start a Session; error state offers Retry", () => {
    const onStart = vi.fn();
    const { unmount } = render(<SessionsField data={snap({ sessions: [] })} onStartSession={onStart} />);
    expect(screen.getByRole("heading", { level: 1, name: "History" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start a Session" }));
    expect(onStart).toHaveBeenCalledTimes(1);
    unmount();
    const onRetry = vi.fn();
    render(<SessionsField data={snap({ sessions: [], unavailableSources: [{ command: "list_sessions", label: "Sensor sessions" }] })} onRetry={onRetry} />);
    expect(screen.getByRole("heading", { level: 1, name: "History" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Sensor sessions unavailable" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
