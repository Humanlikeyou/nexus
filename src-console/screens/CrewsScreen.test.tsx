import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const crew = vi.hoisted(() => ({ startCrew: vi.fn(), getCrewStatus: vi.fn(), stopCrew: vi.fn(), listCrewApprovals: vi.fn(), decideCrewApproval: vi.fn() }));
vi.mock("../bridge", async (orig) => ({ ...(await orig<typeof import("../bridge")>()), ...crew }));

import { INITIAL_SNAPSHOT, crewSnapshot } from "../bridge";
import type { CrewInfo, SystemSnapshot } from "../types";
import { CREW_POLL_MS, CrewsField } from "./CrewsScreen";
import { sampleCrews, unwiredCrews } from "../crews/adapters";

const snap = (c: Partial<SystemSnapshot["crew"]>, extra: Partial<SystemSnapshot> = {}): SystemSnapshot => ({
  ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, crew: { health: "online", runs: [], tools: [], ...c }, ...extra,
});
const run = (over: Partial<CrewInfo> = {}): CrewInfo => ({
  id: "r1", name: "smoke", status: "completed", agents: ["writer"], started_at: "2026-10-28T13:00:00Z",
  task: "Reply with the word OK", result: "OK", ...over,
});

describe("Agents › Crews", () => {
  beforeEach(() => Object.values(crew).forEach((f) => f.mockReset()));
  afterEach(() => vi.useRealTimers());

  it("bridge: crew_health maps to three states; runs only when online", () => {
    expect(crewSnapshot("not_configured", [run()])).toEqual({ health: "not_configured", runs: [], tools: [] });
    expect(crewSnapshot("offline", [run()])).toEqual({ health: "offline", runs: [], tools: [] });
    expect(crewSnapshot("degraded", [run()]).health).toBe("offline");
    expect(crewSnapshot("operational", [run()])).toEqual({ health: "online", runs: [run()], tools: [] });
  });

  it("loading: heading and status, no form", () => {
    render(<CrewsField data={{ ...INITIAL_SNAPSHOT }} />);
    expect(screen.getByRole("heading", { name: "Crews" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading crew service");
    expect(screen.queryByRole("button", { name: "Start run" })).toBeNull();
  });

  it("not configured: badge and the sync-crew-token.sh hint, no runs or form", () => {
    render(<CrewsField data={snap({ health: "not_configured" })} />);
    expect(screen.getAllByText("Not configured").length).toBeGreaterThan(0);
    expect(screen.getByText("scripts/sync-crew-token.sh")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "New Run" })).toBeNull();
  });

  it("offline: badge and Retry", () => {
    const onRetry = vi.fn();
    render(<CrewsField onRetry={onRetry} data={snap({ health: "offline" })} />);
    expect(screen.getByRole("heading", { level: 1, name: "Crews" }).parentElement).toHaveTextContent("Offline");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "New Run" })).toBeNull();
  });

  it("online and empty: Online badge, empty runs note, form", () => {
    render(<CrewsField data={snap({})} />);
    expect(within(screen.getByRole("list", { name: "Crew health" })).getAllByText("Online").length).toBeGreaterThan(0);
    expect(within(screen.getByRole("region", { name: "Runs" })).getByText("No runs yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start run" })).toBeDisabled();
  });

  it("list: rows with clamped task (full text in title), status badges; selecting shows result or error", () => {
    const long = "x".repeat(300);
    render(<CrewsField data={snap({ runs: [run(), run({ id: "r2", name: "broken", status: "failed", task: long, result: "LLM unavailable" }), run({ id: "r3", name: "halted", status: "stopped", result: null })] })} />);
    const runs = screen.getByRole("table", { name: "Runs" });
    expect(within(runs).getByText("Completed")).toBeInTheDocument();
    expect(within(runs).getByText("Failed")).toBeInTheDocument();
    expect(within(runs).getByText("Stopped")).toBeInTheDocument();
    expect(within(runs).getByTitle(long)).toHaveClass("crews__clamp");
    expect(within(runs).getAllByText("writer")).toHaveLength(3);
    // design-1110d D-C5: no source for duration/tokens/cost yet, so the cells are dashes, never numbers.
    expect(within(runs).queryByText(/\d+ s$|tokens|\$/)).toBeNull();
    fireEvent.click(within(runs).getByRole("row", { name: "Open run Broken" }));
    expect(screen.getByText("LLM unavailable", { selector: "pre" })).toHaveClass("crews__result--error");
    expect(screen.getByRole("heading", { level: 1, name: "Crew run" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Crews" }));
    fireEvent.click(within(screen.getByRole("table", { name: "Runs" })).getByRole("row", { name: "Open run Smoke" }));
    expect(screen.getByText("OK", { selector: "pre" })).toHaveClass("crews__result");
  });

  it("list error: crew_list unavailable while online", () => {
    render(<CrewsField data={snap({}, { unavailableSources: [{ command: "crew_list", label: "Crew runs" }] })} />);
    expect(screen.getByText("Crew runs unavailable")).toBeInTheDocument();
  });

  it("new run: validates name, counts task chars, confirms before starting", async () => {
    crew.startCrew.mockResolvedValue(run({ id: "n1", status: "running", result: null }));
    render(<CrewsField data={snap({})} />);
    const nameBox = screen.getByRole("textbox", { name: "Name" });
    fireEvent.change(nameBox, { target: { value: "-bad" } });
    expect(nameBox).toHaveAttribute("aria-invalid", "true");
    fireEvent.change(nameBox, { target: { value: "smoke" } });
    expect(nameBox).not.toHaveAttribute("aria-invalid", "true");
    fireEvent.change(screen.getByRole("textbox", { name: "Task" }), { target: { value: "Reply with the word OK" } });
    expect(screen.getByText("22/4000")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Start run" }));
    const dialog = screen.getByRole("dialog", { name: "Start this run?" });
    expect(dialog).toHaveTextContent("Runs on the local model");
    expect(crew.startCrew).not.toHaveBeenCalled();
    await act(async () => { fireEvent.click(within(dialog).getByRole("button", { name: "Start run" })); });
    expect(crew.startCrew).toHaveBeenCalledWith("smoke", "Reply with the word OK");
    // Decision 0042: starting a run opens that run's page (Stop, live status) with a note, instead of staying on the list.
    expect(screen.getByText(/^Started\./)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "Runs" })).toBeNull();
  });

  it("Retry run starts a new run linked to the old one (retryOf)", async () => {
    crew.startCrew.mockResolvedValue(run({ id: "n2", status: "running", result: null, retry_of: "r1" }));
    render(<CrewsField data={snap({ runs: [run({ status: "failed", result: "boom" })] })} />);
    fireEvent.click(screen.getByRole("row", { name: "Open run Smoke" }));
    fireEvent.click(screen.getByRole("button", { name: "Retry run" }));
    const dialog = screen.getByRole("dialog", { name: "Start this run?" });
    await act(async () => { fireEvent.click(within(dialog).getByRole("button", { name: "Start run" })); });
    expect(crew.startCrew).toHaveBeenCalledWith("smoke", "Reply with the word OK", "r1");
    // ...and lands on the NEW run, not the old one.
    expect(screen.queryByRole("table", { name: "Runs" })).toBeNull();
    expect(screen.getByText(/^Started\./)).toBeInTheDocument();
  });

  it("live crew approvals: listed from the crew service; Approve and Deny call crew_decide_approval", async () => {
    const row = { id: "a1", run_id: "r1", crew: "smoke", agents: ["writer"], kind: "write", title: "Run tool send_note", reason: "the action leaves the Mac", risk: "Medium", requested_at: new Date().toISOString() };
    crew.listCrewApprovals.mockImplementation(async (state: string) => (state === "pending" ? [row] : []));
    crew.decideCrewApproval.mockResolvedValue({ ...row, approved: true });
    await act(async () => { render(<CrewsField data={snap({ runs: [run({ status: "awaiting_approval", result: null })] })} />); });
    expect(await screen.findByText("Run tool send_note")).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Approve Run tool send_note" })); });
    expect(crew.decideCrewApproval).toHaveBeenCalledWith("a1", true);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Deny Run tool send_note" })); });
    expect(crew.decideCrewApproval).toHaveBeenLastCalledWith("a1", false);
  });

  it("a declined native dialog shows its message and approves nothing", async () => {
    const row = { id: "a1", run_id: "r1", crew: "smoke", agents: ["writer"], kind: "write", title: "Run tool send_note", reason: "x", risk: "Low", requested_at: new Date().toISOString() };
    crew.listCrewApprovals.mockImplementation(async (state: string) => (state === "pending" ? [row] : []));
    crew.decideCrewApproval.mockRejectedValue("declined: Denied in the native approval dialog. Nothing was changed.");
    await act(async () => { render(<CrewsField data={snap({ runs: [run()] })} />); });
    await act(async () => { fireEvent.click(await screen.findByRole("button", { name: "Approve Run tool send_note" })); });
    expect(await screen.findByRole("status")).toHaveTextContent(/declined/i);
  });

  it("running: polls crew_status every 5 s while mounted, stops polling when done or unmounted", async () => {
    vi.useFakeTimers();
    crew.getCrewStatus.mockResolvedValueOnce(run({ status: "running", result: null })).mockResolvedValue(run());
    const view = render(<CrewsField data={snap({ runs: [run({ status: "running", result: null })] })} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(CREW_POLL_MS - 1); });
    expect(crew.getCrewStatus).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(crew.getCrewStatus).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(CREW_POLL_MS); });
    expect(crew.getCrewStatus).toHaveBeenCalledTimes(2);
    expect(within(screen.getByRole("table", { name: "Runs" })).getByText("Completed")).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(CREW_POLL_MS * 3); });
    expect(crew.getCrewStatus).toHaveBeenCalledTimes(2);
    view.unmount();

    crew.getCrewStatus.mockReset().mockResolvedValue(run({ status: "running", result: null }));
    const again = render(<CrewsField data={snap({ runs: [run({ status: "running", result: null })] })} />);
    again.unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(CREW_POLL_MS * 3); });
    expect(crew.getCrewStatus).not.toHaveBeenCalled();
  });

  it("stop: destructive confirm, then the row shows the new status", async () => {
    crew.stopCrew.mockResolvedValue("stopped");
    crew.getCrewStatus.mockResolvedValue(run({ status: "stopped", result: null }));
    render(<CrewsField data={snap({ runs: [run({ status: "running", result: null })] })} />);
    fireEvent.click(screen.getByRole("row", { name: "Open run Smoke" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("dialog", { name: "Stop this run?" });
    const confirm = within(dialog).getByRole("button", { name: "Stop Run" });
    expect(crew.stopCrew).not.toHaveBeenCalled();
    await act(async () => { fireEvent.click(confirm); });
    expect(crew.stopCrew).toHaveBeenCalledWith("r1");
    expect(within(screen.getByRole("list", { name: "Run summary" })).getByText("Stopped")).toBeInTheDocument();
  });

  it("start failure shows the sanitized error", async () => {
    crew.startCrew.mockRejectedValue("Crew start failed (429): too many running crews");
    render(<CrewsField data={snap({})} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "smoke" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Task" }), { target: { value: "hi" } });
    fireEvent.click(screen.getByRole("button", { name: "Start run" }));
    await act(async () => { fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Start run" })); });
    expect(screen.getByRole("alert")).toHaveTextContent("too many running crews");
  });
});

describe("Crews › Tools", () => {
  const tools = (_root: HTMLElement) => within(screen.getByRole("region", { name: "Tools and guardrails" }));
  it("lists each tool as allowed or blocked with its limits", () => {
    render(<CrewsField data={snap({ tools: [{ name: "kb_query", allowed: true }, { name: "memory_query", allowed: false }] })} />);
    const t = tools(document.body);
    expect(t.getByText("kb_query")).toBeInTheDocument();
    expect(t.getByLabelText("kb_query: allowed")).toHaveTextContent("Allowed");
    expect(t.getByLabelText("memory_query: blocked")).toHaveTextContent("Blocked");
    expect(t.getAllByText("15 s timeout · 4000-char output cap")).toHaveLength(2);
  });
  it("an empty allowlist: No tools enabled", () => {
    render(<CrewsField data={snap({ tools: [] })} />);
    expect(tools(document.body).getByText("No tools enabled")).toBeInTheDocument();
  });
  it("crew_tools unavailable: Unavailable + Retry", () => {
    const onRetry = vi.fn();
    render(<CrewsField data={snap({}, { unavailableSources: [{ command: "crew_tools", label: "Crew tools" }] })} onRetry={onRetry} />);
    const t = tools(document.body);
    expect(t.getByText("Crew tools unavailable")).toBeInTheDocument();
    fireEvent.click(t.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
  });
  it("loading: no tool list yet", () => {
    render(<CrewsField data={{ ...INITIAL_SNAPSHOT }} />);
    expect(screen.queryByRole("region", { name: "Tools and guardrails" })).toBeNull();
  });
  it("never renders keys, env values or URLs", () => {
    render(<CrewsField data={snap({ tools: [{ name: "kb_query", allowed: true }] })} />);
    expect(screen.getByRole("region", { name: "Tools and guardrails" }).textContent).not.toMatch(/http|CREW_|SECRET|=/);
  });
});


describe("Crews › design-1110d panels", () => {
  const failing = () => snap({ runs: [run(), run({ id: "r2", name: "sample research", status: "failed", task: "List three sample facts", result: "LLM unavailable" })] });
  it("hero names the failed run; unwired panels say Needs wiring and show no sample values", () => {
    render(<CrewsField data={failing()} adapter={unwiredCrews} />);
    expect(screen.getByText("1 of 2 recent runs failed")).toBeInTheDocument();
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThan(2);
    expect(within(screen.getByRole("region", { name: "Crew approvals" })).getByText(/appear here once crews report them/)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "n8n workflows" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("row", { name: "Open run Sample research" }));
    expect(within(screen.getByRole("list", { name: "Run summary" })).queryByText(/\d+ s|local/)).toBeNull();
    expect(screen.queryByRole("table", { name: "Task timeline" })).toBeNull();
  });
  it("sample adapter: values carry a Sample data tag; crew approvals are their own page; no yen", () => {
    render(<CrewsField data={failing()} adapter={sampleCrews} />);
    const queue = screen.getByRole("region", { name: "Crew approvals · 3" });
    expect(within(queue).getByText("Sample data")).toBeInTheDocument();
    expect(within(queue).getByText("Needs wiring · currency conversion")).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/¥/);
    fireEvent.click(within(queue).getByRole("button", { name: "View all ›" }));
    expect(screen.getByText("3 crew approvals waiting")).toBeInTheDocument();
    expect(screen.getByText(/Terrance's approvals stay in chat and Today/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Approve Send weekly supplier email" }));
    expect(screen.getByRole("status")).toHaveTextContent("Sample data: nothing was approved");
    fireEvent.click(screen.getByRole("button", { name: "Crews" }));
    fireEvent.click(screen.getByRole("row", { name: "Open run Sample research" }));
    expect(screen.getByRole("table", { name: "Task timeline" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Handoffs: planner to writer/ })).toBeInTheDocument();
    expect(screen.getAllByText("Sample data").length).toBeGreaterThan(3);
  });
  describe("decision 0042: decisions and starts carry through", () => {
    const row = { id: "a1", run_id: "r1", crew: "smoke", agents: ["writer"], kind: "write", title: "Run tool send_note", reason: "x", risk: "Low", requested_at: new Date().toISOString() };
    const setup = async (onRetry = vi.fn()) => {
      crew.listCrewApprovals.mockImplementation(async (state: string) => (state === "pending" ? [row] : []));
      await act(async () => { render(<CrewsField onRetry={onRetry} data={snap({ runs: [run({ status: "awaiting_approval", result: null })] })} />); });
      return onRetry;
    };

    it("Approve says what happens next, links to the run and refreshes the snapshot", async () => {
      crew.decideCrewApproval.mockResolvedValue({ ...row, approved: true });
      crew.getCrewStatus.mockResolvedValue(run({ status: "running", result: null }));
      const onRetry = await setup();
      await act(async () => { fireEvent.click(await screen.findByRole("button", { name: "Approve Run tool send_note" })); });
      expect(screen.getByRole("status")).toHaveTextContent("The crew is continuing");
      expect(onRetry).toHaveBeenCalledTimes(1);
      fireEvent.click(screen.getByRole("button", { name: "Open run ›" }));
      expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument(); // the run page
    });

    it("Deny says nothing runs and still links and refreshes", async () => {
      crew.decideCrewApproval.mockResolvedValue({ ...row, approved: false });
      const onRetry = await setup();
      await act(async () => { fireEvent.click(await screen.findByRole("button", { name: "Deny Run tool send_note" })); });
      expect(screen.getByRole("status")).toHaveTextContent("Nothing runs");
      expect(onRetry).toHaveBeenCalledTimes(1);
      expect(screen.getByRole("button", { name: "Open run ›" })).toBeInTheDocument();
    });

    it("a declined or failed decision shows the error, no link, no refresh", async () => {
      crew.decideCrewApproval.mockRejectedValue("declined: Denied in the native approval dialog. Nothing was changed.");
      const onRetry = await setup();
      await act(async () => { fireEvent.click(await screen.findByRole("button", { name: "Approve Run tool send_note" })); });
      expect(screen.getByRole("status")).toHaveTextContent(/declined/i);
      expect(screen.queryByRole("button", { name: "Open run ›" })).toBeNull();
      expect(onRetry).not.toHaveBeenCalled();
    });

    it("a failed start stays on the list with the error and opens no run", async () => {
      crew.startCrew.mockRejectedValue("crew service refused");
      render(<CrewsField data={snap({})} />);
      fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "smoke" } });
      fireEvent.change(screen.getByRole("textbox", { name: "Task" }), { target: { value: "Do it" } });
      fireEvent.click(screen.getByRole("button", { name: "Start run" }));
      await act(async () => { fireEvent.click(within(screen.getByRole("dialog", { name: "Start this run?" })).getByRole("button", { name: "Start run" })); });
      expect(screen.getByRole("alert")).toHaveTextContent("crew service refused");
      expect(screen.getByRole("region", { name: "Runs" })).toBeInTheDocument();
    });

    it("openRunId opens that run's page (a link from Agents)", () => {
      render(<CrewsField openRunId="r1" data={snap({ runs: [run()] })} />);
      expect(screen.getByRole("button", { name: /Crews/ })).toBeInTheDocument();
      expect(screen.queryByRole("table", { name: "Runs" })).toBeNull();
    });
  });
});
