import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import { NEEDS_CONVERSION } from "../lib/sampleAdapters";
import type { Strategy, SystemSnapshot } from "../types";
import { BusinessField } from "./BusinessScreen";
import { RevenueField } from "./RevenueScreen";
import { sampleBusiness, unwiredBusiness } from "../business/adapters";
import { resetSampleDecisions } from "../decisions/log";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const strategy: Strategy = { id: "s1", name: "TaiyoSkin shop", type: "e_commerce", status: "active", capital_limit: 5000, daily_loss_cap: 200, total_revenue: 1200, total_expenses: 300, last_health: null, health_status: "healthy" };
const risk = { global_capital_deployed: 2500, global_capital_limit: 10000, global_daily_pnl: -40, global_daily_loss_cap: 500, kill_switch_active: false, active_strategies: 1, strategies_at_risk: ["Ads test"], risk_index: 0.2 };
const revenue = { total_revenue: 1200, total_expenses: 300, net_income: 900, today_revenue: 50, today_expenses: 0, week_revenue: 400, month_revenue: 1200, by_strategy: [{ strategy_id: "s1", strategy_name: "TaiyoSkin shop", revenue: 1200, expenses: 300, net: 900 }] };

describe("Enterprise › Tasks (was Business › Work)", () => {
  it("groups tasks by status, keeps unknown statuses, and the add is disabled with its reason while unwired", () => {
    render(<BusinessField view="tasks" data={snap({ tasks: [{ title: "Ship", status: "active", priority: "high", tags: ["launch_week"] }, { title: "Odd", status: "blocked", priority: null, tags: [] }], pipeline: [] })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Tasks" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "In progress · 1" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Other · 1" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add task" })).toBeDisabled();
    expect(screen.getByText("Needs wiring · task add command")).toBeInTheDocument();
  });
  it("empty and per-source error states", () => {
    const onRetry = vi.fn();
    render(<BusinessField view="tasks" onRetry={onRetry} data={snap({ tasks: [], pipeline: [], unavailableSources: [{ command: "read_tasks", label: "Task source" }] })} />);
    expect(screen.getByText("Task source unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });
  it("old view id 'work' still renders Tasks", () => {
    render(<BusinessField view="work" data={snap({ tasks: [], pipeline: [] })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Tasks" })).toBeInTheDocument();
  });
});

describe("Enterprise › Content", () => {
  it("design-1118a: under 3 items it is a list with one pipeline rail and a next step; nothing posts", () => {
    render(<BusinessField view="content" data={snap({ pipeline: [{ title: "Post", stage: "review", content_type: "blog_post", notes: null }, { title: "Idea", stage: "ideation", content_type: "reel", notes: null }] })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Content" })).toBeInTheDocument();
    expect(screen.getByTestId("content-list")).toBeInTheDocument();
    expect(screen.queryByTestId("content-board")).toBeNull();
    const rail = screen.getByRole("list", { name: "Pipeline stages" });
    for (const st of ["Idea · 1", "Drafting · 0", "Review · 1", "Scheduled · 0", "Published · 0"]) expect(within(rail).getByText(st)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Draft with Amara" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: /publish|post/i })).toBeNull();
  });
  it("is a board from Idea to Published at 3 or more items; the dead Publish button is gone (stage moves live in ContentItemTools)", () => {
    render(<BusinessField view="content" data={snap({ pipeline: [{ title: "Post", stage: "review", content_type: "blog_post", notes: null }, { title: "Idea", stage: "ideation", content_type: "reel", notes: null }, { title: "Draft", stage: "drafting", content_type: "reel", notes: null }] })} />);
    expect(screen.getByTestId("content-board")).toBeInTheDocument();
    for (const col of ["Idea · 1", "Drafting · 1", "Review · 1", "Scheduled · 0", "Published · 0"]) expect(screen.getByRole("region", { name: col })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Publish" })).toBeNull();
    expect(screen.queryByText(/Moving a card between stages needs wiring/)).toBeNull();
  });
});

describe("Enterprise › Decisions (design-1111v §4)", () => {
  it("Record vote writes one decisions-log record, moves no money and invokes no ledger write; Undo removes it", async () => {
    localStorage.setItem("nexus.sampleAdapters", "1");
    resetSampleDecisions();
    vi.mocked(invoke).mockClear();
    const onOpen = vi.fn();
    render(<BusinessField view="decisions" adapter={sampleBusiness} onOpen={onOpen} data={snap({})} />);
    expect(screen.getByRole("heading", { level: 1, name: "Decisions" })).toBeInTheDocument();
    const before = within(screen.getByRole("table", { name: "Decisions log" })).getAllByRole("row").length;
    fireEvent.click(screen.getByRole("button", { name: "Vote yes" }));
    const preview = screen.getByRole("region", { name: "Preview: vote" });
    expect(preview).toHaveTextContent("Nothing moves money. Nothing is sent.");
    expect(preview).toHaveTextContent("Taiyo Skin crew");
    await act(async () => { fireEvent.click(within(preview).getByRole("button", { name: "Record vote" })); });
    expect(within(screen.getByRole("table", { name: "Decisions log" })).getAllByRole("row")).toHaveLength(before + 1);
    expect(screen.getByRole("status")).toHaveTextContent(/Recorded: Yes on first batch/);
    expect(screen.queryByRole("button", { name: "Vote yes" })).toBeNull();
    // No Tauri command at all: nothing reaches the ledger, a bank or anyone outside.
    expect(invoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /Also shows on Taiyo Skin/ }));
    expect(onOpen).toHaveBeenCalledWith({ id: "enterprise", view: "ventures/taiyo-skin" });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /^Undo/ })); });
    expect(within(screen.getByRole("table", { name: "Decisions log" })).getAllByRole("row")).toHaveLength(before);
    expect(screen.getByRole("button", { name: "Vote yes" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ask the accountant" })).toBeDisabled();
    expect(screen.getByText("Amara doesn't contact anyone outside")).toBeInTheDocument();
    localStorage.removeItem("nexus.sampleAdapters");
  });
  it("unwired: votes are disabled with the reason and no log is shown", () => {
    render(<BusinessField view="decisions" adapter={sampleBusiness} data={snap({})} />);
    expect(screen.getByRole("button", { name: "Vote yes" })).toBeDisabled();
    expect(screen.getAllByText("Needs wiring · decisions log").length).toBeGreaterThan(0);
    expect(screen.queryByRole("table", { name: "Decisions log" })).toBeNull();
  });
});

describe("Business › Ventures: Trading (design-1110d D-B5, answer 2)", () => {
  it("shows capital against the limit, P&L against the loss cap, and the kill switch as display-only", () => {
    render(<BusinessField view="ventures" adapter={unwiredBusiness} data={snap({ revenue, strategies: [strategy], risk })} />);
    expect(screen.getByText("Within limits")).toBeInTheDocument();
    // design-1118a: the trading ring + limit tiles (live risk status), display-only.
    const limits = screen.getByRole("list", { name: "Trading limits" });
    expect(within(limits).getByText("-$40 of $500 cap")).toBeInTheDocument();
    expect(within(limits).getByText("$2,500 of $10,000")).toBeInTheDocument();
    expect(within(limits).getByText("0.2")).toBeInTheDocument();
    expect(within(limits).getByText("Off")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "25% of capital deployed" })).toBeInTheDocument();
    expect(screen.getByText(/TaiyoSkin shop/)).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByRole("button", { name: /kill/i })).toBeNull();
  });
  it("kill switch engaged; risk down says so instead of a status", () => {
    const { unmount } = render(<BusinessField view="ventures" adapter={unwiredBusiness} data={snap({ revenue, strategies: [strategy], risk: { ...risk, kill_switch_active: true } })} />);
    expect(screen.getByText("Kill switch engaged")).toBeInTheDocument();
    unmount();
    render(<BusinessField view="ventures" adapter={unwiredBusiness} data={snap({ revenue, strategies: [strategy], unavailableSources: [{ command: "arm_get_risk_status", label: "Revenue risk" }] })} />);
    expect(screen.queryByText(/Kill switch|Within limits/)).toBeNull();
    expect(screen.getByText("Risk unavailable")).toBeInTheDocument();
  });
  it("sample ventures are tagged and open a drill-in; yen values wait for conversion", () => {
    render(<BusinessField view="ventures" adapter={sampleBusiness} data={snap({ revenue, strategies: [strategy], risk })} />);
    expect(screen.getAllByText("Sample data").length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Taiyo Skin" }));
    expect(screen.getByText("Taiyo Skin is at risk")).toBeInTheDocument();
    expect(screen.getAllByText("Needs wiring · currency conversion").length).toBeGreaterThanOrEqual(2);
    expect(document.body.textContent).not.toMatch(/¥/);
  });
});

describe("Business › Overview (design-1110d D-B2–B6)", () => {
  it("leads with real net income; unwired panels show no sample values", () => {
    const onView = vi.fn();
    render(<BusinessField adapter={unwiredBusiness} onView={onView} data={snap({ revenue, strategies: [strategy], risk, tasks: [{ title: "Ship", status: "active", priority: "high", tags: [] }] })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Enterprise" })).toBeInTheDocument();
    expect(screen.getByText("$900 net in 30 days")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Revenue, last 30 days" })).getByText("$1,200")).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.getByRole("region", { name: "Tasks · 1 in progress" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open Tasks ›" }));
    fireEvent.click(screen.getByRole("button", { name: "Open Decisions ›" }));
    expect(onView.mock.calls).toEqual([["tasks"], ["decisions"]]);
  });
});

describe("Enterprise › Finances (was Money; design-1110d D-B9–B11, USD only)", () => {
  it("income vs costs per strategy from the revenue summary; ledger funds wait for currency conversion", () => {
    render(<RevenueField adapter={unwiredBusiness} data={snap({ revenue, strategies: [strategy], risk })} />);
    expect(screen.getByText("$900 net in 30 days")).toBeInTheDocument();
    expect(screen.getByLabelText("$1,200 income, $300 costs")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Finances" })).toBeInTheDocument();
    expect(screen.getByText(/Read-only, from the business ledger/)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "In Decisions" })).toBeInTheDocument();
    // design-1118a: no rows of dashes; fund rows stay hidden until a fund has a balance.
    expect(screen.queryByRole("list", { name: "Fund balances" })).toBeNull();
    expect(screen.getByText(/Fund balances appear here once the ledger reports one/)).toBeInTheDocument();
    expect(screen.getByText("Needs wiring · currency conversion")).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByText(/Kill switch|Within limits/)).toBeNull();
    expect(document.body.textContent).not.toMatch(/¥/);
  });
  it("awaiting state", () => {
    render(<RevenueField adapter={unwiredBusiness} data={snap({ strategies: [strategy] })} />);
    expect(screen.getByRole("heading", { level: 2, name: "No revenue yet" })).toBeInTheDocument();
    expect(screen.getByText(/Tracking starts when a real source reports in\./)).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "0% revenue" })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Fund balances" })).toBeNull();
  });
  it("fund vessels appear once a fund has a balance; Akiya never shows here", () => {
    const a = { ...unwiredBusiness, funds: () => ({ state: "live" as const, value: [{ name: "Unallocated", balanceUsd: { state: "live" as const, value: 160 } }, { name: "Akiya", balanceUsd: { state: "live" as const, value: 50 } }, { name: "Taiyo Skin", balanceUsd: NEEDS_CONVERSION }] }) };
    render(<RevenueField adapter={a} data={snap({ strategies: [strategy] })} />);
    const list = screen.getByRole("list", { name: "Fund balances" });
    expect(within(list).getByText("Unallocated")).toBeInTheDocument();
    expect(within(list).queryByText("Akiya")).toBeNull();
    expect(within(list).queryByText("Taiyo Skin")).toBeNull();
  });
});
