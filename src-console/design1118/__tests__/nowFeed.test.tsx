import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { INITIAL_SNAPSHOT } from "../../bridge";
import type { SystemSnapshot } from "../../types";
import { NowFeedView, buildNowRows, lastFinishedAt, sinceText } from "../NowFeed";

const NOW = Date.parse("2026-11-19T10:00:00Z");
const iso = (minAgo: number) => new Date(NOW - minAgo * 60_000).toISOString();
const snap = (over: Partial<SystemSnapshot> = {}): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, ...over, crew: { ...INITIAL_SNAPSHOT.crew, health: "offline", ...(over.crew ?? {}) } });
const action = (over: Record<string, unknown>) => ({ id: "x", toolId: "system.open_url", title: "Open https://secret.example/?token=ABC", status: "executing", requiresApproval: true, parameters: { url: "https://secret.example" }, conversationId: null, contextItemIds: [], requestedAt: iso(5), updatedAt: iso(5), approvedAt: null, startedAt: iso(3), completedAt: null, result: null, error: null, retryOfActionId: null, retryAllowed: false, summary: "SECRET SUMMARY", ...over }) as unknown as SystemSnapshot["actions"][number];

describe("Working now feed", () => {
  it("shows one row per agent with who, what, since and status", () => {
    const data = snap({
      crew: { health: "online", tools: [], runs: [{ id: "r1", name: "Taiyo Skin", status: "running", agents: [], started_at: iso(10), task: "SECRET PROMPT BODY", result: null }, { id: "r2", name: "Writers", status: "awaiting_approval", agents: [], started_at: iso(2), task: null, result: null }] },
      actions: [action({ id: "a1" })],
    });
    render(<NowFeedView data={data} now={NOW} backend={{ items: [{ kind: "amara", who: "Amara", what: "Answering you", status: "running", sinceMs: NOW - 30_000 }] }} jobs={[{ id: "j1", type: "custom_job", name: "Morning note", status: "running", startedAt: NOW - 120_000 }]} />);
    const feed = screen.getByTestId("now-feed");
    expect(within(feed).getByText("Crew: Taiyo Skin")).toBeTruthy();
    expect(within(feed).getByText(/Answering you · since just now/)).toBeTruthy();
    expect(within(feed).getByText("Automation")).toBeTruthy();
    expect(within(feed).getByText("Waiting for you")).toBeTruthy();
    expect(within(feed).getAllByText("Running").length).toBe(4);
    expect(within(feed).getByText(/Working on a task · since 10 min ago/)).toBeTruthy();
  });

  it("sorts waiting first", () => {
    const data = snap({ crew: { health: "online", tools: [], runs: [{ id: "r1", name: "A", status: "running", agents: [], started_at: iso(10), task: null, result: null }, { id: "r2", name: "B", status: "awaiting_approval", agents: [], started_at: iso(1), task: null, result: null }] } });
    expect(buildNowRows(data, { backend: null, jobs: null })[0].who).toBe("Crew: B");
  });

  it("nothing running: one calm line with when the last thing finished", () => {
    const data = snap({ activity: [{ id: "e", type: "t", name: "n", status: "complete", startedAt: NOW - 9_000_000, completedAt: NOW - 7_200_000, durationMs: 1, detail: null, outcome: "success" }] });
    render(<NowFeedView data={data} now={NOW} backend={{ items: [] }} jobs={[]} />);
    expect(screen.getByTestId("now-empty").textContent).toBe("Nobody is working right now. The last thing finished 2 h ago.");
    expect(lastFinishedAt(snap())).toBeNull();
  });

  it("nothing running and nothing ever finished: no invented time", () => {
    render(<NowFeedView data={snap()} now={NOW} backend={{ items: [] }} jobs={[]} />);
    expect(screen.getByTestId("now-empty").textContent).toBe("Nobody is working right now.");
  });

  it("all sources unreadable: Needs wiring, no rows", () => {
    const down = ["list_terrence_actions", "get_outbox_status", "list_activity_entries"].map((command) => ({ command, label: command }));
    render(<NowFeedView data={snap({ unavailableSources: down })} now={NOW} backend={null} jobs={null} />);
    expect(screen.getByText("Needs wiring")).toBeTruthy();
    expect(screen.queryByTestId("now-feed")).toBeNull();
    expect(screen.queryByTestId("now-empty")).toBeNull();
  });

  it("a source that cannot be read is absent, others still show", () => {
    const data = snap({ unavailableSources: [{ command: "list_terrence_actions", label: "x" }], actions: [action({ id: "a1" })] });
    const rows = buildNowRows(data, { backend: null, jobs: null });
    expect(rows).toEqual([]);
  });

  it("folds rows beyond 6", () => {
    const items = Array.from({ length: 9 }, (_, i) => ({ kind: "amara", who: `Amara ${i}`, what: "Answering you", status: "running", sinceMs: NOW - i * 1000 }));
    render(<NowFeedView data={snap()} now={NOW} backend={{ items }} jobs={[]} />);
    const feed = screen.getByTestId("now-feed");
    expect(within(feed).getByText("3 more")).toBeTruthy();
    expect(buildNowRows(snap(), { backend: { items }, jobs: [] }).length).toBe(9);
    expect(feed.querySelectorAll("details").length).toBe(1);
    expect(feed.querySelectorAll("details li, details [class*=list-row]").length).toBeGreaterThanOrEqual(3);
  });

  it("never renders prompt bodies, action titles or summaries; conversation chat actions are skipped", () => {
    const data = snap({
      crew: { health: "online", tools: [], runs: [{ id: "r1", name: "Taiyo Skin", status: "running", agents: [], started_at: iso(1), task: "SECRET PROMPT BODY", result: "SECRET RESULT" }] },
      actions: [action({ id: "a1" }), action({ id: "a2", toolId: "agent.hermes_chat", title: "SECRET CHAT TEXT" })],
      outbox: { ...INITIAL_SNAPSHOT.outbox, queue_depth: 2 },
    });
    const { container } = render(<NowFeedView data={data} now={NOW} backend={{ items: [] }} jobs={[]} />);
    const text = container.textContent ?? "";
    for (const bad of ["SECRET", "token=", "secret.example"]) expect(text).not.toContain(bad);
    expect(text).toContain("A task on your Mac");
    expect(text).toContain("2 waiting to send");
  });

  it("relative time text", () => {
    expect(sinceText(null, NOW)).toBeNull();
    expect(sinceText(NOW - 5 * 60_000, NOW)).toBe("5 min ago");
    expect(sinceText(NOW - 3 * 86_400_000, NOW)).toBe("3 d ago");
  });
});
