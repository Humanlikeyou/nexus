import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { fireEvent } from "@testing-library/react";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { ActivityField } from "./ActivityScreen";
import { activityBadge } from "./businessTone";

const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const outbox = { gateway_online: true, queue_depth: 2, last_delivered_at: "2026-10-28T13:40:00+09:00", last_error: "timeout", total_delivered: 41 };

describe("Chat & Agents › Activity (§10)", () => {
  it("delivery card and activity rows", () => {
    render(<ActivityField data={snap({ outbox, activity: [{ id: "a1", type: "mac_action", name: "open_app", status: "completed", startedAt: Date.parse("2026-10-28T13:00:00+09:00"), completedAt: 0, durationMs: 1200, detail: "Opened Notes", outcome: null }] })} />);
    expect(screen.getByText("Gateway delivery")).toBeInTheDocument();
    expect(screen.getByText("Gateway online")).toBeInTheDocument();
    expect(screen.getByText("timeout")).toBeInTheDocument();
    expect(screen.getByText("open app")).toBeInTheDocument();
    expect(screen.getByText("Mac action · Opened Notes")).toBeInTheDocument();
        expect(document.querySelector(".signal-stream")).toBeNull();
  });
  it("expired items: shown as Expired, hidden when none", () => {
    const { unmount } = render(<ActivityField data={snap({ outbox: { ...outbox, queue_depth: 0, last_error: null, expired: 11 }, activity: [] })} />);
    expect(screen.getByText("Expired")).toBeInTheDocument();
    expect(screen.getByText("11")).toBeInTheDocument();
    expect(screen.getByText("Older than 24h, kept, not sent")).toBeInTheDocument();
    unmount();
    render(<ActivityField data={snap({ outbox, activity: [] })} />);
    expect(screen.queryByText("Expired")).toBeNull();
  });
  it("empty and error states", () => {
    const onRetry = vi.fn();
    render(<ActivityField onRetry={onRetry} data={snap({ outbox, activity: [], unavailableSources: [{ command: "list_activity_entries", label: "Activity log" }] })} />);
    expect(screen.getByText("Activity log unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });
});

describe("Activity status badges (Decision 20 tones)", () => {
  it.each([
    ["complete", "success", "Complete"],
    ["completed", "success", "Completed"],
    ["skipped", "neutral", "Skipped"],
    ["suppressed", "neutral", "Suppressed"],
    ["cancelled", "neutral", "Cancelled"],
    ["failed", "danger", "Failed"],
    ["error", "danger", "Failed"],
    ["running", "accent", "Running"],
    ["pending", "warning", "Pending"],
  ])("%s → %s '%s'", (status, tone, label) => {
    expect(activityBadge(status)).toEqual({ tone, label });
    const { unmount } = render(<ActivityField data={snap({ outbox, activity: [{ id: "x", type: "ritual", name: "Ritual: Test", status, startedAt: Date.parse("2026-10-28T13:00:00+09:00"), completedAt: 0, durationMs: 0, detail: null, outcome: null }] })} />);
    const badge = screen.getByText("Ritual: Test").closest("li")?.querySelector(".ui-badge");
    expect(badge?.textContent).toBe(label);
    expect(badge?.getAttribute("data-tone")).toBe(tone);
    unmount();
  });

  it("row time uses dateTime(): 'Oct 28, 1:00 PM'", () => {
    render(<ActivityField data={snap({ outbox, activity: [{ id: "a1", type: "mac_action", name: "open_app", status: "completed", startedAt: Date.parse("2026-10-28T13:00:00+09:00"), completedAt: 0, durationMs: 1200, detail: "Opened Notes", outcome: null }] })} />);
    expect(screen.getByText("1.2s · Oct 28, 1:00 PM")).toBeInTheDocument();
    expect(screen.getByText("Last delivered Oct 28, 1:40 PM")).toBeInTheDocument();
  });
});
