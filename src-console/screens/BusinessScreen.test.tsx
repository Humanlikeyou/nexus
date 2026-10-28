import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { Strategy, SystemSnapshot } from "../types";
import { BusinessField } from "./BusinessScreen";
import { RevenueField } from "./RevenueScreen";

const snap = (extra: Partial<SystemSnapshot>): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra });
const strategy: Strategy = { id: "s1", name: "TaiyoSkin shop", type: "e_commerce", status: "active", capital_limit: 5000, daily_loss_cap: 200, total_revenue: 1200, total_expenses: 300, last_health: null, health_status: "healthy" };
const risk = { global_capital_deployed: 2500, global_capital_limit: 10000, global_daily_pnl: -40, global_daily_loss_cap: 500, kill_switch_active: false, active_strategies: 1, strategies_at_risk: ["Ads test"], risk_index: 0.2 };
const revenue = { total_revenue: 1200, total_expenses: 300, net_income: 900, today_revenue: 50, today_expenses: 0, week_revenue: 400, month_revenue: 1200, by_strategy: [{ strategy_id: "s1", strategy_name: "TaiyoSkin shop", revenue: 1200, expenses: 300, net: 900 }] };

describe("Business › Work (§7)", () => {
  it("counts statuses incl. Other, lists work items and pipeline stages", () => {
    render(<BusinessField data={snap({ tasks: [{ title: "Ship", status: "active", priority: "high", tags: ["launch_week"] }, { title: "Odd", status: "blocked", priority: null, tags: [] }], pipeline: [{ title: "Post", stage: "drafting", content_type: "blog_post", notes: "n" }] })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Business" })).toBeInTheDocument();
    expect(screen.getByText("Other")).toBeInTheDocument();
    expect(screen.getByText("2 tracked")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Drafting · 1" })).toBeInTheDocument();
  });
  it("empty and per-source error states", () => {
    const onRetry = vi.fn();
    render(<BusinessField onRetry={onRetry} data={snap({ tasks: [], pipeline: [], unavailableSources: [{ command: "read_tasks", label: "Task source" }] })} />);
    expect(screen.getByText("Task source unavailable")).toBeInTheDocument();
    expect(screen.getByText("Pipeline is clear")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });
});

describe("Business › Revenue (§8)", () => {
  it("labels capital_limit as a limit, shows P&L against the loss cap, kill switch is display-only", () => {
    render(<RevenueField data={snap({ revenue, strategies: [strategy], risk })} />);
    expect(screen.getByText(/\$5,000 limit/)).toBeInTheDocument();
    expect(screen.queryByText(/\$[0-9,]+ deployed/)).toBeNull();
    expect(screen.getByText("Daily P&L -$40 of $500 loss cap · Risk index 0.2")).toBeInTheDocument();
    expect(screen.getByText("Risk within limits")).toBeInTheDocument();
    expect(screen.getByText("Ads test")).toBeInTheDocument();
    expect(screen.getByRole("meter", { name: "Capital deployed" })).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByText("ARM")).toBeNull();
  });
  it("kill switch engaged; risk down hides the badge", () => {
    const { unmount } = render(<RevenueField data={snap({ revenue, strategies: [strategy], risk: { ...risk, kill_switch_active: true } })} />);
    expect(screen.getByText("Kill switch engaged")).toBeInTheDocument();
    unmount();
    render(<RevenueField data={snap({ revenue, strategies: [strategy], unavailableSources: [{ command: "arm_get_risk_status", label: "Revenue risk" }] })} />);
    expect(screen.queryByText(/Kill switch|within limits/)).toBeNull();
    expect(screen.getAllByText("Revenue risk unavailable").length).toBeGreaterThan(0);
  });
  it("awaiting state", () => {
    render(<RevenueField data={snap({ strategies: [strategy] })} />);
    const card = screen.getByRole("region", { name: "No revenue yet" });
    expect(within(card).getByText("Tracking starts when a real source reports in.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Configured strategies (1)" })).toBeInTheDocument();
  });
});
