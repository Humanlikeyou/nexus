import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const agg = vi.hoisted(() => ({ list: vi.fn() }));
const follow = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("../lib/localStores", async (orig) => ({ ...(await orig<typeof import("../lib/localStores")>()), inTauri: () => true, needsYouList: agg.list }));
vi.mock("../bridge", async (orig) => ({ ...(await orig<typeof import("../bridge")>()), listFollowups: follow.list }));

import { INITIAL_SNAPSHOT } from "../bridge";
import type { TerrenceAction } from "../types";
import { NeedsYouToday, useNeedsYou } from "./NeedsYouToday";
import { FOCUS_ACTION_EVENT } from "./needsYou";

const NOW = Date.parse("2026-11-19T03:00:00Z");
const data = { ...INITIAL_SNAPSHOT, refreshedAt: NOW };
const lists = { open: [], recentlyDone: [], mutedTopics: [] } as never;

beforeEach(() => { agg.list.mockReset(); follow.list.mockReset(); follow.list.mockResolvedValue(lists); });

describe("G12 Needs you Try again", () => {
  it("re-reads the aggregator as well as the follow-ups", async () => {
    agg.list.mockResolvedValue([{ source: "crew", state: "failed", rows: [] }]);
    const { result } = renderHook(() => useNeedsYou(data, NOW));
    await waitFor(() => expect(agg.list).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(follow.list).toHaveBeenCalledTimes(1));
    await new Promise((r) => setTimeout(r, 20));
    const aggBefore = agg.list.mock.calls.length, followBefore = follow.list.mock.calls.length;
    act(() => result.current.retry());
    await waitFor(() => expect(agg.list.mock.calls.length).toBeGreaterThan(aggBefore));
    expect(follow.list.mock.calls.length).toBeGreaterThan(followBefore);
  });
  it("a failed source shows the line, and Try again recovers when the source answers", async () => {
    agg.list.mockResolvedValueOnce([{ source: "crew", state: "failed", rows: [] }]).mockResolvedValue([{ source: "crew", state: "ok", rows: [] }]);
    function Probe() { const inbox = useNeedsYou(data, NOW); return <NeedsYouToday inbox={inbox} />; }
    render(<Probe />);
    expect(await screen.findByText(/didn.t load\. The rest is current\./)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.queryByText(/didn.t load\./)).toBeNull());
  });
});

describe("G4 Open on Home for an Amara approval", () => {
  const pending = { id: "act-7", toolId: "macos.open_url", title: "Open the dashboard", status: "awaiting_approval" } as unknown as TerrenceAction;
  it("announces the action id (App expands the chat) and still opens Home", async () => {
    agg.list.mockResolvedValue([]);
    const onOpen = vi.fn();
    const seen: unknown[] = [];
    const on = (e: Event) => seen.push((e as CustomEvent).detail);
    window.addEventListener(FOCUS_ACTION_EVENT, on);
    function Probe() { const inbox = useNeedsYou({ ...data, actions: [pending] }, NOW); return <NeedsYouToday inbox={inbox} onOpen={onOpen} />; }
    render(<Probe />);
    fireEvent.click(await screen.findByRole("button", { name: /Open the dashboard/ }));
    window.removeEventListener(FOCUS_ACTION_EVENT, on);
    expect(seen).toEqual([{ id: "act-7" }]);
    expect(onOpen).toHaveBeenCalledWith({ id: "terrance", view: "main" });
  });
});

describe("0045 habits rows in Needs you", () => {
  it("shows the Habits label and 'Open habits ›', and links to life/goals", async () => {
    agg.list.mockResolvedValue([{ source: "habits", state: "ok", rows: [{ source: "habits", id: "habit-h1", title: "Stretch", due: null, reason: "Not marked today", deepLink: { id: "life", view: "goals" }, quiet: true }] }]);
    const onOpen = vi.fn();
    function Probe() { const inbox = useNeedsYou(data, NOW); return <NeedsYouToday inbox={inbox} onOpen={onOpen} />; }
    render(<Probe />);
    const btn = await screen.findByRole("button", { name: /Open habits: Stretch/ });
    expect(btn).toHaveTextContent("Open habits ›");
    expect(screen.getByText("Habits", { selector: ".v-src" })).toBeInTheDocument();
    fireEvent.click(btn);
    expect(onOpen).toHaveBeenCalledWith({ id: "life", view: "goals" });
  });
});
