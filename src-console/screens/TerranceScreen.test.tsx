import { fireEvent, render, screen, within } from "@testing-library/react";
import { act } from "react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { TerranceField, greeting, homeDate, homeSummary, lastRunLabel, nextIn, nextRuns, relative, statusStrip } from "./TerranceScreen";

const NOW = new Date("2026-10-28T14:00:00+09:00").getTime();
const H = 3_600_000;
const running = { running: true, lastConsolidation: NOW - 4 * H + 60_000 * 14, lastReasoning: new Date("2026-10-28T08:12:00+09:00").getTime() };

function snap(extra: Partial<SystemSnapshot> = {}): SystemSnapshot {
  return { ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...extra };
}

const full = snap({
  hermes: { ...INITIAL_SNAPSHOT.hermes, status: "online", model: "qwen3", provider: "ollama", gatewayRunning: true, telegramStatus: "connected" },
  noop: { ...INITIAL_SNAPSHOT.noop, status: "ready", today: { day: "2026-10-28", source: "noop", totalSleepMin: 452, efficiency: 0.9, deepMin: 90, remMin: 100, lightMin: 230, restingHr: 52, avgHrv: 64, recovery: 74, strain: 9, spo2Pct: null, skinTempDevC: null, respRateBpm: null }, insights: [{ metric: "hrv", fact: "HRV is up 8%", context: "Above your 14-day mean." }] },
  prioritySnapshot: { ...INITIAL_SNAPSHOT.prioritySnapshot, items: [{ rank: 1, sourceKind: "project", sourceId: "p1", title: "Ship Amara OS v2", sourceStatus: "active" } as SystemSnapshot["prioritySnapshot"]["items"][number]] },
  sessions: [{ id: "s1", device_type: "muse", start: "2026-10-27T21:00:00+09:00", end: "2026-10-27T21:20:00+09:00", duration_secs: 1200, data_points: 10, summary: {} }],
});

describe("Home (TerranceField)", () => {
  it("revenue uses grouped currency", () => {
    render(<TerranceField data={{ ...full, revenue: { ...full.revenue, today_revenue: 1820, net_income: -12345.6 } }} now={NOW} scheduler={running} />);
    const revenue = screen.getByRole("region", { name: "Revenue today" });
    expect(revenue).toHaveTextContent("$1,820");
    expect(revenue).toHaveTextContent("-$12,346");
  });

  it("neuro: Calm shows the saved calm_pct, rounded, with the Beta badge", () => {
    const session = { ...full.sessions[0], summary: { calm_pct: 61.6, time_in_target_pct: 40 } };
    render(<TerranceField data={{ ...full, sessions: [session] }} now={NOW} scheduler={running} />);
    const neuro = screen.getByRole("region", { name: "Neuro" });
    expect(neuro).toHaveTextContent("Calm Beta62%");
    expect(within(neuro).getByText("Beta")).toBeInTheDocument();
  });

  it("loading: skeleton cards, heading, no dashes", () => {
    const { container } = render(<TerranceField data={INITIAL_SNAPSHOT} now={NOW} scheduler={running} />);
    expect(screen.getByRole("heading", { level: 1, name: "Command Center" })).toBeInTheDocument();
    expect(screen.getByText("Good afternoon, Taisun")).toBeInTheDocument();
    // Loading: date only, no "All clear" before data lands.
    expect(screen.getByText("Wednesday, Oct 28")).toBeInTheDocument();
    expect(container.textContent).not.toContain("All clear");
    expect(container.querySelector("[aria-busy=true]")).not.toBeNull();
    expect(container.textContent).not.toContain("—");
  });

  it("ready: recovery ring, scheduler next/last, insight, agents, priorities, neuro last session, footer", () => {
    const onNavigate = vi.fn();
    render(<TerranceField data={full} now={NOW} scheduler={running} onNavigate={onNavigate} />);
    expect(screen.getByRole("meter", { name: "Recovery" })).toHaveAttribute("aria-valuenow", "74");
    expect(within(screen.getByRole("region", { name: "Scheduler" })).getByText("Next: Consolidation in 2h 14m")).toBeInTheDocument();
    expect(screen.getByText("Last: Reasoning 8:12 AM")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(screen.getByText("HRV is up 8%")).toBeInTheDocument();
    expect(screen.queryByText("Gateway online")).toBeNull(); // folded into the status strip's Agents segment
    const agents = screen.getByRole("region", { name: "Agents" });
    expect(within(agents).getByText("Hermes Agent")).toHaveAttribute("title", "qwen3");
    expect(within(agents).queryByText(/Hermes Agent ·/)).toBeNull();
    // OpenClaw isn't the active backend here: no Assistant row and no OpenClaw label.
    expect(within(agents).queryByText("Assistant")).toBeNull();
    expect(within(agents).queryByText(/OpenClaw|Not connected/)).toBeNull();
    expect(screen.getByText("Ship Amara OS v2")).toBeInTheDocument();
    const neuro = screen.getByRole("region", { name: "Neuro" });
    expect(neuro).toHaveTextContent("Last session20min");
    // A session saved before calm_pct existed: Calm shows "—" + Beta (Decision 5).
    expect(neuro).toHaveTextContent("Calm Beta—");
    expect(within(neuro).getByText("Beta")).toBeInTheDocument();
    // Recovery band word is colored by band; header status is sentence-case text, not a badge.
    expect(screen.getByText("High")).toHaveAttribute("data-band", "high");
    // Every linkable card uses the same "Open ›" header accessory; no in-body buttons.
    for (const name of ["Open Fitness", "Open insights in Fitness", "Open Agents", "Open chat on Home", "Open Neuro", "Open Revenue"]) {
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
    expect(screen.queryByText("Stopped")).toBeNull(); // no duplicate badge under the strip
    expect(screen.getByText("Background jobs aren't running.")).toBeInTheDocument();
    expect(screen.getByText("Your first completed session will appear here.")).toBeInTheDocument();
    expect(screen.queryByRole("contentinfo")).toBeNull();
  });

  it("error: per-card unavailable with Retry and a header count", () => {
    const onRetry = vi.fn();
    render(<TerranceField onRetry={onRetry} now={NOW} scheduler={running} data={snap({ unavailableSources: [{ command: "get_noop_health_snapshot", label: "NOOP health" }, { command: "get_terrence_runtime_health", label: "Amara runtime" }] })} />);
    expect(screen.getByText("2 sources unavailable")).toHaveAttribute("title", "NOOP health, Amara runtime");
    expect(screen.getAllByText("NOOP health unavailable")).toHaveLength(2);
    expect(screen.getByText("Amara runtime unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Retry" })[0]);
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("helpers", () => {
    expect(nextRuns({ running: true, lastConsolidation: 0, lastReasoning: null })).toEqual({ consolidation: 6 * H, reasoning: null });
    expect(nextRuns({ running: true, lastConsolidation: 0, lastReasoning: 0, nextConsolidationAt: 5, nextReasoningAt: 7 })).toEqual({ consolidation: 5, reasoning: 7 });
    expect(relative(NOW + 30_000, NOW)).toBe("now");
    expect(relative(NOW - 2 * H, NOW)).toBe("2h 0m ago");
    // Next runs never read "ago": an overdue job is due now (live check: "Next: Consolidation 2744h 42m ago").
    expect(nextIn(NOW - 2744 * H, NOW)).toBe("due now");
    expect(nextIn(NOW + 30_000, NOW)).toBe("due now");
    expect(nextIn(NOW + 2 * H + 14 * 60_000, NOW)).toBe("in 2h 14m");
    expect(nextIn(null, NOW)).toBeNull();
    // Last runs on another day carry the date (live check: July 6 read as "Last: Reasoning 13:54").
    const at = (iso: string) => Date.parse(iso);
    const today = at("2026-10-29T10:58:00+09:00");
    expect(lastRunLabel(at("2026-10-29T08:05:00+09:00"), today)).toBe("8:05 AM");
    expect(lastRunLabel(at("2026-07-06T13:54:00+09:00"), today)).toBe("Jul 6, 1:54 PM");
    expect(lastRunLabel(at("2026-10-28T23:30:00+09:00"), today)).toBe("Oct 28, 11:30 PM");
    expect(lastRunLabel(at("2025-12-31T09:00:00+09:00"), today)).toBe("Dec 31, 2025, 9:00 AM");
    expect(greeting(new Date("2026-10-28T08:00:00+09:00").getTime())).toBe("Good morning");
    expect(greeting(new Date("2026-10-28T20:00:00+09:00").getTime())).toBe("Good evening");
  });

  describe("subline (Decision 13)", () => {
    const action = (status: string) => ({ id: status, toolId: "mac.open_app", title: "Open Notes", status }) as unknown as SystemSnapshot["actions"][number];

    it("formats the date as weekday, short month and day in Tokyo, from the given clock", () => {
      expect(homeDate(new Date("2026-10-29T10:21:00+09:00").getTime())).toBe("Thursday, Oct 29");
      // 23:30 UTC on the 28th is already the 29th in Tokyo.
      expect(homeDate(Date.parse("2026-10-28T23:30:00Z"))).toBe("Thursday, Oct 29");
    });

    it("picks actions waiting, then priorities, then all clear; nothing when sources are down", () => {
      expect(homeSummary(snap({ actions: [action("awaiting_approval"), action("awaiting_approval"), action("succeeded")] }))).toEqual({ kind: "actions", text: "Amara has 2 actions waiting for approval" });
      expect(homeSummary(snap({ actions: [action("awaiting_approval")] }))?.text).toBe("Amara has 1 action waiting for approval");
      expect(homeSummary(full)).toEqual({ kind: "priorities", text: "Amara is tracking 1 priority today" });
      expect(homeSummary(snap())).toEqual({ kind: "clear", text: "All clear. Amara is standing by." });
      expect(homeSummary(snap({ unavailableSources: [{ command: "list_terrence_actions", label: "Action ledger" }] }))).toBeNull();
    });

    it("renders the status · date line; actions waiting is a button to Chat", () => {
      const onNavigate = vi.fn();
      const { rerender, container } = render(<TerranceField data={full} now={NOW} scheduler={running} onNavigate={onNavigate} />);
      expect(container.querySelector(".home__summary")?.textContent).toBe("Amara is tracking 1 priority today · Wednesday, Oct 28");
      expect(container.querySelector(".home__summary time")).toHaveAttribute("dateTime", "2026-10-28");
      rerender(<TerranceField data={{ ...full, actions: [action("awaiting_approval")] }} now={NOW} scheduler={running} onNavigate={onNavigate} />);
      fireEvent.click(screen.getByRole("button", { name: "Amara has 1 action waiting for approval" }));
      expect(onNavigate).toHaveBeenCalledWith("chat");
      rerender(<TerranceField data={snap()} now={NOW} scheduler={running} onNavigate={onNavigate} />);
      expect(container.querySelector(".home__summary")?.textContent).toBe("All clear. Amara is standing by. · Wednesday, Oct 28");
    });
  });
});

describe("Home status strip (Command center feel)", () => {
  const strip = () => screen.getByRole("group", { name: "Status" });
  const segment = (key: string) => strip().querySelector<HTMLButtonElement>(`[data-key="${key}"]`)!;
  const fresh = { ...full, noop: { ...full.noop, generatedAt: new Date(NOW - 6 * 60_000).toISOString() } };

  it("three plain-text segments with dots, no Badge inside; live ones pulse", () => {
    render(<TerranceField data={fresh} now={NOW} scheduler={running} onNavigate={vi.fn()} />);
    expect(strip().querySelector(".ui-badge")).toBeNull();
    expect(segment("agents")).toHaveTextContent(/^Amara online$/);
    expect(segment("scheduler")).toHaveTextContent("Next: Consolidation in 2h 14m");
    expect(segment("scheduler")).toHaveAttribute("title", "Oct 28, 4:14 PM");
    expect(segment("noop")).toHaveTextContent("NOOP synced 6 min ago");
    expect(segment("noop")).toHaveAttribute("title", "Oct 28, 1:54 PM");
    for (const key of ["agents", "scheduler", "noop"]) {
      expect(segment(key).tagName).toBe("BUTTON");
      expect(segment(key).querySelector(".ui-status-dot")).toHaveAttribute("data-tone", "success");
    }
    for (const key of ["agents", "scheduler"]) expect(segment(key)).toHaveAttribute("data-live");
    expect(segment("noop")).not.toHaveAttribute("data-live"); // nightly sync, not a live item
  });

  it("a ready NOOP sync older than 2 h uses the warning tone and never pulses", () => {
    const old = { ...full, noop: { ...full.noop, status: "ready" as const, generatedAt: new Date(NOW - 6 * H).toISOString() } };
    const seg = statusStrip(old, running, NOW)[2];
    expect([seg.text, seg.tone, seg.live]).toEqual(["NOOP synced 6 h ago", "warning", false]);
    const recent = statusStrip(fresh, running, NOW)[2];
    expect([recent.tone, recent.live]).toEqual(["success", false]);
  });

  it("deep links: Agents → Agents, Scheduler → Settings, NOOP → Fitness", () => {
    const onNavigate = vi.fn();
    render(<TerranceField data={fresh} now={NOW} scheduler={running} onNavigate={onNavigate} />);
    for (const key of ["agents", "scheduler", "noop"]) fireEvent.click(segment(key));
    expect(onNavigate.mock.calls.map((c) => c[0])).toEqual(["chat", "settings", "health"]);
  });

  it("Background jobs stopped; Amara offline; NOOP stale (yellow) and unavailable (red), always with the word", () => {
    const offline = { ...full, hermes: { ...full.hermes, status: "offline" as const }, noop: { ...full.noop, status: "stale" as const, generatedAt: new Date(NOW - 2 * 86_400_000).toISOString() } };
    const segs = statusStrip(offline, { running: false, lastConsolidation: null, lastReasoning: null }, NOW);
    expect(segs.map((x) => [x.key, x.text, x.tone, x.live])).toEqual([
      ["agents", "Amara offline", "danger", false],
      ["scheduler", "Background jobs stopped", "danger", false],
      ["noop", "NOOP stale · synced 2 d ago", "warning", false],
    ]);
    const down = statusStrip({ ...full, noop: { ...full.noop, status: "unavailable" } }, running, NOW)[2];
    expect([down.text, down.tone, down.live]).toEqual(["NOOP unavailable", "danger", false]);
  });

  it("the soonest of consolidation, reasoning and daily summary is shown", () => {
    const s = { ...running, nextConsolidationAt: NOW + 3 * H, nextReasoningAt: NOW + 5 * H, nextDailySummaryAt: NOW + 40 * 60_000 };
    expect(statusStrip(full, s, NOW)[1].text).toBe("Next: Daily summary in 40m");
  });
});

describe("NOOP stale tone on the strip's own 30s tick", () => {
  it("turns warning once the sync passes 2 h, without a parent re-render", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(NOW);
      const data = { ...full, noop: { ...full.noop, status: "ready" as const, generatedAt: new Date(NOW - 2 * H + 20_000).toISOString() } };
      render(<TerranceField data={data} scheduler={running} onNavigate={vi.fn()} />);
      const dot = () => screen.getByRole("group", { name: "Status" }).querySelector('[data-key="noop"] .ui-status-dot')!;
      expect(dot()).toHaveAttribute("data-tone", "success");
      act(() => { vi.advanceTimersByTime(30_000); });
      expect(dot()).toHaveAttribute("data-tone", "warning");
    } finally { vi.useRealTimers(); }
  });
});

describe("Home: Assistant badge", () => {
  const oc = (status: "online" | "configured" | "not_installed") => ({ status, online: status === "online", model: "minimax-m2.5", gateway_url: "", last_heartbeat: null });
  it("backend reachable: an 'Amara' row with an aria-label, strip says connected", () => {
    const on = { ...full, openclaw: oc("online") };
    render(<TerranceField data={on} now={NOW} scheduler={running} onNavigate={vi.fn()} />);
    const agents = screen.getByRole("region", { name: "Agents" });
    expect(within(agents).getByLabelText("Amara status: Online")).toBeInTheDocument();
    expect(within(agents).getByText("minimax-m2.5")).toBeInTheDocument();
    expect(statusStrip(on, running, NOW)[0].text).toBe("Amara online · Assistant connected");
    expect(document.body.textContent).not.toMatch(/OpenClaw/);
  });
  it("status unreadable: neutral 'Assistant' (ambiguous)", () => {
    render(<TerranceField data={{ ...full, unavailableSources: [{ command: "read_openclaw_status", label: "Assistant backend" }] }} now={NOW} scheduler={running} onNavigate={vi.fn()} />);
    expect(within(screen.getByRole("region", { name: "Agents" })).getByLabelText("Assistant status: Checking")).toBeInTheDocument();
  });
  it("installed but silent: 'Assistant backend not reachable' + Retry (warning), no raw error", () => {
    const onRetry = vi.fn();
    render(<TerranceField data={{ ...full, openclaw: oc("configured") }} now={NOW} scheduler={running} onNavigate={vi.fn()} onRetry={onRetry} />);
    const agents = screen.getByRole("region", { name: "Agents" });
    const block = within(agents).getByRole("status");
    expect(block).toHaveTextContent("Assistant backend not reachable");
    expect(block).toHaveAttribute("data-tone", "warning");
    fireEvent.click(within(block).getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
    expect(agents.textContent).not.toMatch(/18789|launchd|OpenClaw/);
  });
});

describe("Home: Knowledge base card", () => {
  const kbCard = () => screen.getByRole("region", { name: "Knowledge base" });
  const withKb = (kb: SystemSnapshot["kb"], extra: Partial<SystemSnapshot> = {}) =>
    render(<TerranceField data={{ ...full, kb, ...extra }} now={NOW} scheduler={running} onNavigate={vi.fn()} onRetry={vi.fn()} />);
  it("ready: document count and model", () => {
    withKb({ state: "running", model: "nomic-embed-text", docs: 8, detail: null });
    expect(within(kbCard()).getByLabelText("Knowledge base status: Ready")).toBeInTheDocument();
    expect(kbCard()).toHaveTextContent("8 documents · nomic-embed-text");
  });
  it("empty: 'Knowledge base is empty' + help line, no Retry", () => {
    withKb({ state: "empty", model: "nomic-embed-text", docs: 0, detail: "knowledge base is empty" });
    expect(kbCard()).toHaveTextContent("Knowledge base is empty");
    expect(kbCard()).toHaveTextContent("Add documents to search them.");
    expect(within(kbCard()).queryByRole("button", { name: "Retry" })).toBeNull();
  });
  it("not running: warning 'not running' with Retry", () => {
    withKb({ state: "not_running", model: null, docs: null, detail: null });
    expect(within(kbCard()).getByRole("status")).toHaveAttribute("data-tone", "warning");
    expect(kbCard()).toHaveTextContent("Knowledge base: not running");
    expect(within(kbCard()).getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
  it("error: danger 'query failed'; backend detail is never shown", () => {
    withKb({ state: "error", model: null, docs: null, detail: "AioRpcError StatusCode.UNKNOWN 172.18.0.4" });
    expect(within(kbCard()).getByRole("status")).toHaveAttribute("data-tone", "danger");
    expect(kbCard()).toHaveTextContent("Knowledge base: query failed");
    expect(kbCard().textContent).not.toMatch(/AioRpcError|StatusCode|172\.18/);
  });
  it("kb_status unavailable: warning + Retry", () => {
    withKb({ state: "running", model: null, docs: 3, detail: null }, { unavailableSources: [{ command: "kb_status", label: "Knowledge base" }] });
    expect(within(kbCard()).getByLabelText("Knowledge base status: Unavailable")).toBeInTheDocument();
  });
  it("loading: Home shows its skeleton, no KB card yet", () => {
    render(<TerranceField data={{ ...INITIAL_SNAPSHOT }} now={NOW} scheduler={running} onNavigate={vi.fn()} />);
    expect(screen.queryByRole("region", { name: "Knowledge base" })).toBeNull();
  });
});

