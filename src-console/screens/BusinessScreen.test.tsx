import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { Strategy, SystemSnapshot } from "../types";
import { BusinessField } from "./BusinessScreen";
import { RevenueField } from "./RevenueScreen";
import { sampleBusiness, unwiredBusiness } from "../business/adapters";

const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const strategy: Strategy = { id: "s1", name: "TaiyoSkin shop", type: "e_commerce", status: "active", capital_limit: 5000, daily_loss_cap: 200, total_revenue: 1200, total_expenses: 300, last_health: null, health_status: "healthy" };
const risk = { global_capital_deployed: 2500, global_capital_limit: 10000, global_daily_pnl: -40, global_daily_loss_cap: 500, kill_switch_active: false, active_strategies: 1, strategies_at_risk: ["Ads test"], risk_index: 0.2 };
const revenue = { total_revenue: 1200, total_expenses: 300, net_income: 900, today_revenue: 50, today_expenses: 0, week_revenue: 400, month_revenue: 1200, by_strategy: [{ strategy_id: "s1", strategy_name: "TaiyoSkin shop", revenue: 1200, expenses: 300, net: 900 }] };

describe("Business › Work (§7)", () => {
  it("counts statuses incl. Other, lists work items and pipeline stages", () => {
    render(<BusinessField view="work" data={snap({ tasks: [{ title: "Ship", status: "active", priority: "high", tags: ["launch_week"] }, { title: "Odd", status: "blocked", priority: null, tags: [] }], pipeline: [{ title: "Post", stage: "drafting", content_type: "blog_post", notes: "n" }] })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Business" })).toBeInTheDocument();
    expect(screen.getByText("Other")).toBeInTheDocument();
    expect(screen.getByText("2 tracked")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Drafting · 1" })).toBeInTheDocument();
  });
  it("empty and per-source error states", () => {
    const onRetry = vi.fn();
    render(<BusinessField view="work" onRetry={onRetry} data={snap({ tasks: [], pipeline: [], unavailableSources: [{ command: "read_tasks", label: "Task source" }] })} />);
    expect(screen.getByText("Task source unavailable")).toBeInTheDocument();
    expect(screen.getByText("Pipeline is clear. Content moves here from idea to publication.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });
});

describe("Business › Ventures: Trading (design-1110d D-B5, answer 2)", () => {
  it("shows capital against the limit, P&L against the loss cap, and the kill switch as display-only", () => {
    render(<BusinessField view="ventures" adapter={unwiredBusiness} data={snap({ revenue, strategies: [strategy], risk })} />);
    expect(screen.getByText("Within limits")).toBeInTheDocument();
    expect(screen.getByText(/daily P&L -\$40 of \$500 loss cap · kill switch off/)).toBeInTheDocument();
    expect(screen.getByText("Capital $2,500 of $10,000 deployed · risk index 0.2")).toBeInTheDocument();
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
    render(<BusinessField adapter={unwiredBusiness} data={snap({ revenue, strategies: [strategy], risk, tasks: [{ title: "Ship", status: "active", priority: "high", tags: [] }] })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Business" })).toBeInTheDocument();
    expect(screen.getByText("$900 net in 30 days")).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Revenue, last 30 days" })).getByText("$1,200")).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.getByRole("region", { name: "Work · 1 in progress" })).toBeInTheDocument();
  });
});

describe("Business › Money (design-1110d D-B9–B11, USD only)", () => {
  it("income vs costs per strategy from the revenue summary; ledger funds wait for currency conversion", () => {
    render(<RevenueField adapter={unwiredBusiness} data={snap({ revenue, strategies: [strategy], risk })} />);
    expect(screen.getByText("$900 net in 30 days")).toBeInTheDocument();
    expect(screen.getByLabelText("$1,200 income, $300 costs")).toBeInTheDocument();
    const funds = screen.getByRole("table", { name: "Fund balances" });
    expect(within(funds).getAllByText("\u2014")).toHaveLength(5);
    expect(screen.getByText("Needs wiring · currency conversion")).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByText(/Kill switch|Within limits/)).toBeNull();
    expect(document.body.textContent).not.toMatch(/¥/);
  });
  it("awaiting state", () => {
    render(<RevenueField adapter={unwiredBusiness} data={snap({ strategies: [strategy] })} />);
    expect(screen.getByText("No revenue yet")).toBeInTheDocument();
    expect(screen.getByText("Tracking starts when a real source reports in.")).toBeInTheDocument();
  });
});
