import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import { ASK_MORE_EVENT, type AskMoreDetail } from "../amaraTake/takeView";
import { sampleBusiness } from "../business/adapters";
import { resetSampleDecisions } from "../decisions/log";
import { UNWIRED } from "../lib/sampleAdapters";
import { ContentItemTools, VentureGoalsCard } from "../design1118/EnterpriseWiring";
import { ProposalsCard, SourceLink, WatchlistCard } from "../design1118/Trading";
import type { EnterpriseLive } from "../design1118/enterpriseLive";
import { makeLiveMarket, type LiveMarket } from "../design1118/marketProposalsLive";
import type { SystemSnapshot } from "../types";
import { BusinessField } from "./BusinessScreen";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async (c: string) => (c === "add_history" || c === "decisions_list" ? [] : null)) }));
const base = async (c: string) => (c === "add_history" || c === "decisions_list" ? [] : null);

const live = <T,>(value: T) => ({ state: "live" as const, value });
const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const listen = () => { const got: AskMoreDetail[] = []; const on = (e: Event) => got.push((e as CustomEvent<AskMoreDetail>).detail); window.addEventListener(ASK_MORE_EVENT, on); return { got, off: () => window.removeEventListener(ASK_MORE_EVENT, on) }; };

afterEach(() => { localStorage.removeItem("nexus.sampleAdapters"); delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; vi.useRealTimers(); resetSampleDecisions(); vi.mocked(invoke).mockReset(); vi.mocked(invoke).mockImplementation(base); });

describe("Enterprise flows: venture page", () => {
  beforeEach(() => { localStorage.setItem("nexus.sampleAdapters", "1"); resetSampleDecisions(); });
  it("Life › Dates & deadlines routes to Life dates, not Tasks", () => {
    const onOpen = vi.fn();
    render(<BusinessField view="ventures" adapter={sampleBusiness} data={snap({})} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button", { name: /Life › Dates/ }));
    expect(onOpen).toHaveBeenCalledWith({ id: "life", view: "dates" });
  });
  it("Draft with crew opens Home chat pre-filled with the venture's next step; the vote link opens that item in Decisions", () => {
    const onView = vi.fn();
    const l = listen();
    render(<BusinessField view="ventures/taiyo-skin" adapter={sampleBusiness} data={snap({})} onView={onView} onOpen={vi.fn()} />);
    fireEvent.click(screen.getAllByRole("button", { name: "Draft with crew" })[0]);
    expect(l.got).toHaveLength(1);
    expect(l.got[0].question).toMatch(/Taiyo Skin/);
    expect(l.got[0].chips.map((c) => c.label)).toEqual(["Venture", "Next step"]);
    l.off();
    const vote = screen.queryByRole("button", { name: "Open in Decisions ›" });
    if (vote) { fireEvent.click(vote); expect(onView.mock.calls[0][0]).toMatch(/^decisions\/.+/); }
  });
  it("a linked goal opens Life › Goals", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const a = { ventures: () => live([{ id: "taiyo-skin", goals: [{ id: "g1", title: "Launch", stepsTotal: 4, stepsDone: 1, percent: 25 }] }]), linkGoal: vi.fn(), undoGoalLink: vi.fn() } as unknown as EnterpriseLive;
    const onOpen = vi.fn();
    render(<VentureGoalsCard ventureId="taiyo-skin" adapter={a} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button", { name: "Open goal in Life: Launch" }));
    expect(onOpen).toHaveBeenCalledWith({ id: "life", view: "goals" });
  });
});

describe("Enterprise flows: Decisions", () => {
  beforeEach(() => { localStorage.setItem("nexus.sampleAdapters", "1"); resetSampleDecisions(); });
  it("a decisions/<id> view highlights and keeps that waiting item; an unknown id says so", () => {
    const first = sampleBusiness.fundingRequests();
    const id = first.state === "unwired" ? "" : first.value[0].id;
    const { container, unmount } = render(<BusinessField view={`decisions/${id}`} adapter={sampleBusiness} data={snap({})} />);
    expect(container.querySelector(`#dec-${id}`)?.className).toContain("v-new");
    unmount();
    render(<BusinessField view="decisions/nope" adapter={sampleBusiness} data={snap({})} />);
    expect(screen.getByText(/That item isn't waiting any more/)).toBeInTheDocument();
  });
  it("with no venture page for the requester, the record links to the Ventures list and says so", async () => {
    const onOpen = vi.fn();
    render(<BusinessField view="decisions" adapter={{ ...sampleBusiness, ventures: () => UNWIRED }} onOpen={onOpen} data={snap({})} />);
    fireEvent.click(screen.getByRole("button", { name: "Vote yes" }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Record vote" })); });
    fireEvent.click(screen.getByRole("button", { name: /No venture page for this yet/ }));
    expect(onOpen).toHaveBeenCalledWith({ id: "enterprise", view: "ventures" });
  });
  it("a failed write says That didn't save and keeps the form", async () => {
    localStorage.removeItem("nexus.sampleAdapters");
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockImplementation(async (c: string) => { if (c === "decisions_list") return []; if (c === "decisions_append") throw new Error("disk"); return null; });
    const view = <BusinessField view="decisions" adapter={sampleBusiness} data={snap({})} />;
    const { rerender } = render(view);
    await waitFor(() => expect(vi.mocked(invoke).mock.calls.some((c) => c[0] === "decisions_list")).toBe(true));
    await act(async () => { await Promise.resolve(); });
    rerender(<BusinessField view="decisions" adapter={sampleBusiness} data={snap({ refreshedAt: 9e15 })} />);
    fireEvent.click(await screen.findByRole("button", { name: "Vote yes" }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Record vote" })); });
    expect(await screen.findByRole("alert")).toHaveTextContent("That didn't save. Nothing was recorded.");
    expect(screen.getByRole("region", { name: "Preview: vote" })).toBeInTheDocument();
  });
});

describe("Enterprise flows: Tasks and Content", () => {
  it("a task row's Ask Amara opens Home chat pre-filled with the task", () => {
    const l = listen();
    render(<BusinessField view="tasks" data={snap({ tasks: [{ title: "Ship labels", status: "active", priority: null, tags: [] }], pipeline: [] })} />);
    fireEvent.click(screen.getByRole("button", { name: "Ask Amara about Ship labels" }));
    expect(l.got[0].question).toContain("Ship labels");
    expect(l.got[0].sample).toBe(false);
    l.off();
  });
  it("the board puts stage moves on every card (Published asks Did you post it?) and removes the disabled Publish", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const pipeline = [{ title: "A", stage: "review", content_type: "blog_post", notes: null }, { title: "B", stage: "ideation", content_type: "reel", notes: null }, { title: "C", stage: "drafting", content_type: "reel", notes: null }];
    render(<BusinessField view="content" data={snap({ pipeline })} />);
    expect(screen.getAllByTestId("content-tools")).toHaveLength(3);
    expect(screen.queryByRole("button", { name: "Publish" })).toBeNull();
    fireEvent.change(screen.getByLabelText("Move “A” to"), { target: { value: "published" } });
    expect(screen.getByRole("group", { name: "Did you post it?" })).toBeInTheDocument();
  });
  it("a saved idea is stored through the live add path and the snapshot reloads (no local sample row)", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const preview = { title: "Reel idea", fields: [["Type", "Short video"]] };
    vi.mocked(invoke).mockImplementation(async (c: string) => c === "add_preview" ? { kind: "preview", preview } : c === "add_save" ? { id: "idea-1", title: "Reel idea" } : c === "add_history" ? [] : null);
    const onRetry = vi.fn();
    render(<BusinessField view="content" onRetry={onRetry} data={snap({ pipeline: [] })} />);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Reel: how label approval works" } });
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Add idea" })[0]).toBeEnabled());
    fireEvent.click(screen.getAllByRole("button", { name: "Add idea" })[0]);
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Add idea" })).toHaveLength(2));
    fireEvent.click(screen.getAllByRole("button", { name: "Add idea" })[1]);
    await waitFor(() => expect(onRetry).toHaveBeenCalled());
    expect(screen.queryByText(/sample data/)).toBeNull();
  });
});

describe("ContentItemTools: saved draft", () => {
  it("loads the saved draft, offers Redraft, and after saving offers the next stage move, then reloads", async () => {
    const a = {
      savedDraft: vi.fn(async () => live({ title: "T", text: "Earlier draft", file: "f" })),
      draft: vi.fn(async () => live({ title: "T", text: "New draft", tier: "local" })),
      saveDraft: vi.fn(async () => live({ id: "d1", title: "T", file: "f", bytes: 1, toast: "Draft saved." })),
      setStage: vi.fn(async () => live({ id: "c1", title: "T", from: "drafting", to: "review", toast: "Moved to Review." })),
      undoDraft: vi.fn(), undoStage: vi.fn(),
    } as unknown as EnterpriseLive;
    const onChanged = vi.fn();
    render(<ContentItemTools title="T" stage="drafting" adapter={a} onChanged={onChanged} />);
    expect(await screen.findByText("Earlier draft")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Redraft" }));
    expect(await screen.findByText("New draft")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save draft" }));
    fireEvent.click(await screen.findByRole("button", { name: "Move to Review" }));
    await waitFor(() => expect(a.setStage).toHaveBeenCalledWith("T", "drafting", "review", false));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(await screen.findByText("Moved to Review.")).toBeInTheDocument();
  });
  it("a failed save says so and keeps the draft", async () => {
    const a = { savedDraft: vi.fn(async () => UNWIRED), draft: vi.fn(async () => live({ title: "T", text: "D", tier: "local" })), saveDraft: vi.fn(async () => UNWIRED) } as unknown as EnterpriseLive;
    render(<ContentItemTools title="T" stage="ideation" adapter={a} />);
    fireEvent.click(screen.getByRole("button", { name: "Draft with Amara" }));
    fireEvent.click(await screen.findByRole("button", { name: "Save draft" }));
    expect(await screen.findByText(/The draft didn't save/)).toBeInTheDocument();
    expect(screen.getByText("D")).toBeInTheDocument();
  });
});

describe("Trading flows", () => {
  it("approving reloads the watchlist and says Now on your watchlist; Recently decided lists it", async () => {
    const calls: string[] = [];
    const call = vi.fn(async (c: string) => {
      calls.push(c);
      if (c === "watch_proposals_list") return { proposals: [{ id: "p1", createdAt: "x", kind: "stock", side: "watch", ticker: "ABC", reasonText: "Why", sourceName: "Src", confidence: "low", riskText: "Risk", status: calls.includes("watch_proposal_record") ? "recorded" : "proposed", decidedAt: "2026-11-19T10:00:00+09:00" }] };
      if (c === "watchlist_get") return { picks: [], feed: { state: "connected" }, research: { running: false } };
      return { id: "h1", toast: "Recorded." };
    }) as never;
    const adapter = makeLiveMarket({ call, notify: () => {} });
    const { rerender } = render(<ProposalsCard adapter={adapter} />);
    await waitFor(() => expect(adapter.proposals().state).toBe("live"));
    rerender(<ProposalsCard adapter={adapter} key="2" />);
    fireEvent.click(await screen.findByRole("button", { name: "Approve" }));
    fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));
    expect(await screen.findByText(/Now on your watchlist/)).toBeInTheDocument();
    expect(calls.filter((c) => c === "watchlist_get").length).toBeGreaterThan(0);
    const recent = await screen.findByTestId("recently-decided");
    expect(within(recent).getByText("Watch ABC")).toBeInTheDocument();
    expect(recent.textContent).toContain("No order was sent");
  });
  it("Research now polls while running (bounded), then reports N new picks", async () => {
    vi.useFakeTimers();
    let running = false;
    const w = () => ({ picks: [], feed: { state: "connected" }, research: { running, lastProposed: 2, lastFinishedAt: running ? null : "t" }, reviewDue: false, disclaimer: "" });
    const refresh = vi.fn(async () => { running = false; });
    const researchNow = vi.fn(async () => { running = true; return live({ state: "started", retryAfterMinutes: null }); });
    const a = { watchlist: () => live(w()), researchNow, refresh } as unknown as LiveMarket;
    render(<WatchlistCard adapter={a} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Research now" })); });
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(screen.getByText(/Amara found 2 new picks/)).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(refresh).toHaveBeenCalledTimes(1);
  });
  it("Research with nothing found says No new picks; a run that never finishes stops polling after the bound", async () => {
    vi.useFakeTimers();
    let running = false;
    const proposed = 0;
    const w = () => ({ picks: [], feed: { state: "connected" }, research: { running, lastProposed: proposed, lastFinishedAt: running ? null : "t" }, reviewDue: false, disclaimer: "" });
    const refresh = vi.fn(async () => undefined);
    const researchNow = vi.fn(async () => { running = true; return live({ state: "started", retryAfterMinutes: null }); });
    const a = { watchlist: () => live(w()), researchNow, refresh } as unknown as LiveMarket;
    const { unmount } = render(<WatchlistCard adapter={a} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Research now" })); });
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000 * 40); });
    expect(refresh.mock.calls.length).toBeLessThanOrEqual(30);
    expect(screen.getByText(/taking a while/)).toBeInTheDocument();
    unmount();
    running = false;
    render(<WatchlistCard adapter={a} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Research now" })); });
    running = false;
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(screen.getByText("No new picks.")).toBeInTheDocument();
  });
  it("a pick's source is an https link opened by click only; other schemes render as text", () => {
    const { container, rerender } = render(<SourceLink name="Reuters" url="https://example.com/a" />);
    const a = container.querySelector("a");
    expect(a?.getAttribute("href")).toBe("https://example.com/a");
    expect(a?.getAttribute("rel")).toContain("noopener");
    expect(a?.getAttribute("target")).toBe("_blank");
    rerender(<SourceLink name="Reuters" url="http://example.com/a" />);
    expect(container.querySelector("a")).toBeNull();
    rerender(<SourceLink name="Reuters" url="javascript:alert(1)" />);
    expect(container.querySelector("a")).toBeNull();
    rerender(<SourceLink name="Reuters" url={null} />);
    expect(container.textContent).toBe("Reuters");
  });
  it("L3: URLs with credentials render as plain text; a normal link shows its hostname", () => {
    const { container, rerender } = render(<SourceLink name="Reuters" url="https://good.com@evil.com/" />);
    expect(container.querySelector("a")).toBeNull();
    expect(container.textContent).toBe("Reuters");
    rerender(<SourceLink name="Reuters" url="https://u:p@evil.com/" />);
    expect(container.querySelector("a")).toBeNull();
    rerender(<SourceLink name="Reuters" url="https://example.com/a" />);
    expect(container.querySelector("a")).not.toBeNull();
    expect(container.textContent).toContain("example.com");
  });
});
