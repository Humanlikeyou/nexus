import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { TerranceField, greeting, nextRuns, relative } from "./TerranceScreen";

const NOW = new Date("2026-10-28T14:00:00+09:00").getTime();
const H = 3_600_000;
const running = { running: true, lastConsolidation: NOW - 4 * H + 60_000 * 14, lastReasoning: new Date("2026-10-28T08:12:00+09:00").getTime() };

function snap(extra: Partial<SystemSnapshot> = {}): SystemSnapshot {
  return { ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra };
}

const full = snap({
  hermes: { ...INITIAL_SNAPSHOT.hermes, status: "online", model: "qwen3", provider: "ollama", gatewayRunning: true, telegramStatus: "connected" },
  noop: { ...INITIAL_SNAPSHOT.noop, status: "ready", today: { day: "2026-10-28", source: "noop", totalSleepMin: 452, efficiency: 0.9, deepMin: 90, remMin: 100, lightMin: 230, restingHr: 52, avgHrv: 64, recovery: 74, strain: 9, spo2Pct: null, skinTempDevC: null, respRateBpm: null }, insights: [{ metric: "hrv", fact: "HRV is up 8%", context: "Above your 14-day mean." }] },
  prioritySnapshot: { ...INITIAL_SNAPSHOT.prioritySnapshot, items: [{ rank: 1, sourceKind: "project", sourceId: "p1", title: "Ship Nexus v2", sourceStatus: "active" } as SystemSnapshot["prioritySnapshot"]["items"][number]] },
  sessions: [{ id: "s1", device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:20:00+09:00", duration_secs: 1200, data_points: 10, summary: {} }],
});

describe("Home (TerranceField)", () => {
  it("revenue uses grouped currency", () => {
    render(<TerranceField data={{ ...full, revenue: { ...full.revenue, today_revenue: 1820, net_income: -12345.6 } }} now={NOW} scheduler={running} />);
    const revenue = screen.getByRole("region", { name: "Revenue today" });
    expect(revenue).toHaveTextContent("$1,820");
    expect(revenue).toHaveTextContent("-$12,346");
  });

  it("neuro: Calm shows the saved calm_pct, rounded, with the BETA badge", () => {
    const session = { ...full.sessions[0], summary: { calm_pct: 61.6, time_in_target_pct: 40 } };
    render(<TerranceField data={{ ...full, sessions: [session] }} now={NOW} scheduler={running} />);
    const neuro = screen.getByRole("region", { name: "Neuro" });
    expect(neuro).toHaveTextContent("Calm BETA62%");
    expect(within(neuro).getByText("BETA")).toBeInTheDocument();
  });

  it("loading: skeleton cards, heading, no dashes", () => {
    const { container } = render(<TerranceField data={INITIAL_SNAPSHOT} now={NOW} scheduler={running} />);
    expect(screen.getByRole("heading", { level: 1, name: "Good afternoon, Taisun" })).toBeInTheDocument();
    expect(container.querySelector("[aria-busy=true]")).not.toBeNull();
    expect(container.textContent).not.toContain("—");
  });

  it("ready: recovery ring, scheduler next/last, insight, agents, priorities, neuro last session, footer", () => {
    const onNavigate = vi.fn();
    render(<TerranceField data={full} now={NOW} scheduler={running} onNavigate={onNavigate} />);
    expect(screen.getByRole("meter", { name: "Recovery" })).toHaveAttribute("aria-valuenow", "74");
    expect(screen.getByText("Next: Consolidation in 2h 14m")).toBeInTheDocument();
    expect(screen.getByText("Last: Reasoning 08:12")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(screen.getByText("HRV is up 8%")).toBeInTheDocument();
    expect(screen.getByText("Gateway online")).toBeInTheDocument();
    const agents = screen.getByRole("region", { name: "Agents" });
    expect(within(agents).getByText("qwen3 · ollama")).toBeInTheDocument();
    expect(within(agents).getByText("Not connected")).toBeInTheDocument();
    expect(screen.getByText("Ship Nexus v2")).toBeInTheDocument();
    const neuro = screen.getByRole("region", { name: "Neuro" });
    expect(neuro).toHaveTextContent("Last session20min");
    // A session saved before calm_pct existed: Calm shows "—" + BETA (Decision 5).
    expect(neuro).toHaveTextContent("Calm BETA—");
    expect(within(neuro).getByText("BETA")).toBeInTheDocument();
    // Recovery band word is colored by band; header status is sentence-case text, not a badge.
    expect(screen.getByText("High")).toHaveAttribute("data-band", "high");
    expect(screen.getByText("Gateway online").closest(".home__status")).toHaveAttribute("data-tone", "success");
    // Every linkable card uses the same "Open ›" header accessory; no in-body buttons.
    for (const name of ["Open Fitness", "Open insights in Fitness", "Open Chat & Agents", "Open Chat", "Open Neuro", "Open Revenue"]) {
      expect(screen.getByRole("button", { name })).toHaveTextContent("Open");
    }
    expect(screen.getByText(/Scheduler · next consolidation 2h 14m · next reasoning 6h 12m/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open Fitness" }));
    expect(onNavigate).toHaveBeenCalledWith("health");
    expect(document.body.textContent).not.toMatch(/whoop/i);
  });

  it("empty: no recovery, insights, priorities; scheduler stopped", () => {
    render(<TerranceField data={snap({ noop: { ...INITIAL_SNAPSHOT.noop, message: "No local store yet." } })} now={NOW} scheduler={{ running: false, lastConsolidation: null, lastReasoning: null }} />);
    expect(screen.getByText("No recovery yet · No local store yet.")).toBeInTheDocument();
    expect(screen.getByText("Insights appear after NOOP's nightly sync.")).toBeInTheDocument();
    expect(screen.getByText("No current priorities")).toBeInTheDocument();
    expect(screen.getByText("Stopped")).toBeInTheDocument();
    expect(screen.getByText("Background jobs aren't running.")).toBeInTheDocument();
    expect(screen.getByText("Your first completed session will appear here.")).toBeInTheDocument();
    expect(screen.queryByRole("contentinfo")).toBeNull();
  });

  it("error: per-card unavailable with Retry and a header count", () => {
    const onRetry = vi.fn();
    render(<TerranceField onRetry={onRetry} now={NOW} scheduler={running} data={snap({ unavailableSources: [{ command: "get_noop_health_snapshot", label: "NOOP health" }, { command: "get_terrence_runtime_health", label: "Terrence runtime" }] })} />);
    expect(screen.getByText("2 sources unavailable")).toHaveAttribute("title", "NOOP health, Terrence runtime");
    expect(screen.getAllByText("NOOP health unavailable")).toHaveLength(2);
    expect(screen.getByText("Terrence runtime unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Retry" })[0]);
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("helpers", () => {
    expect(nextRuns({ running: true, lastConsolidation: 0, lastReasoning: null })).toEqual({ consolidation: 6 * H, reasoning: null });
    expect(nextRuns({ running: true, lastConsolidation: 0, lastReasoning: 0, nextConsolidationAt: 5, nextReasoningAt: 7 })).toEqual({ consolidation: 5, reasoning: 7 });
    expect(relative(NOW + 30_000, NOW)).toBe("now");
    expect(relative(NOW - 2 * H, NOW)).toBe("2h 0m ago");
    expect(greeting(new Date("2026-10-28T08:00:00+09:00").getTime())).toBe("Good morning");
    expect(greeting(new Date("2026-10-28T20:00:00+09:00").getTime())).toBe("Good evening");
  });
});
