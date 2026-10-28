import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { fireEvent } from "@testing-library/react";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { ActivityField } from "./ActivityScreen";

const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const outbox = { gateway_online: true, queue_depth: 2, last_delivered_at: "2026-10-28T13:40:00+09:00", last_error: "timeout", total_delivered: 41 };

describe("Chat & Agents › Activity (§10)", () => {
  it("delivery card and activity rows", () => {
    render(<ActivityField data={snap({ outbox, activity: [{ id: "a1", type: "mac_action", name: "open_app", status: "completed", startedAt: Date.parse("2026-10-28T13:00:00+09:00"), completedAt: 0, durationMs: 1200, detail: "Opened Notes", outcome: null }] })} />);
    expect(screen.getByRole("region", { name: "Telegram delivery" })).toBeInTheDocument();
    expect(screen.getByText("Gateway online")).toBeInTheDocument();
    expect(screen.getByText("timeout")).toBeInTheDocument();
    expect(screen.getByText("open app")).toBeInTheDocument();
    expect(screen.getByText("Mac action · Opened Notes")).toBeInTheDocument();
    expect(screen.getByText("Last 30 operations")).toBeInTheDocument();
    expect(document.querySelector(".signal-stream")).toBeNull();
  });
  it("empty and error states", () => {
    const onRetry = vi.fn();
    render(<ActivityField onRetry={onRetry} data={snap({ outbox, activity: [], unavailableSources: [{ command: "list_activity_entries", label: "Activity log" }] })} />);
    expect(screen.getByText("Activity log unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });
});
