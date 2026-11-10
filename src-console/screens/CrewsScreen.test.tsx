import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const crew = vi.hoisted(() => ({ startCrew: vi.fn(), getCrewStatus: vi.fn(), stopCrew: vi.fn() }));
vi.mock("../bridge", async (orig) => ({ ...(await orig<typeof import("../bridge")>()), ...crew }));

import { INITIAL_SNAPSHOT, crewSnapshot } from "../bridge";
import type { CrewInfo, SystemSnapshot } from "../types";
import { CREW_POLL_MS, CrewsField } from "./CrewsScreen";

const snap = (c: Partial<SystemSnapshot["crew"]>, extra: Partial<SystemSnapshot> = {}): SystemSnapshot => ({
  ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, crew: { health: "online", runs: [], tools: [], ...c }, ...extra,
});
const run = (over: Partial<CrewInfo> = {}): CrewInfo => ({
  id: "r1", name: "smoke", status: "completed", agents: ["writer"], started_at: "2026-10-28T13:00:00Z",
  task: "Reply with the word OK", result: "OK", ...over,
});

describe("Chat & Agents › Crews", () => {
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
    expect(screen.getByText("Not configured")).toBeInTheDocument();
    expect(screen.getByText("scripts/sync-crew-token.sh")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "New Run" })).toBeNull();
  });

  it("offline: badge and Retry", () => {
    const onRetry = vi.fn();
    render(<CrewsField onRetry={onRetry} data={snap({ health: "offline" })} />);
    expect(screen.getByText("Offline")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "New Run" })).toBeNull();
  });

  it("online and empty: Online badge, empty runs note, form", () => {
    render(<CrewsField data={snap({})} />);
    expect(screen.getByText("Online")).toBeInTheDocument();
    expect(screen.getByText("No runs yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start run" })).toBeDisabled();
  });

  it("list: rows with clamped task (full text in title), status badges; selecting shows result or error", () => {
    const long = "x".repeat(300);
    render(<CrewsField data={snap({ runs: [run(), run({ id: "r2", name: "broken", status: "failed", task: long, result: "LLM unavailable" }), run({ id: "r3", name: "halted", status: "stopped", result: null })] })} />);
    const runs = screen.getByRole("list", { name: "Runs" });
    expect(within(runs).getByText("Completed")).toBeInTheDocument();
    expect(within(runs).getByText("Failed")).toBeInTheDocument();
    expect(within(runs).getByText("Stopped")).toBeInTheDocument();
    expect(within(runs).getByTitle(long)).toHaveClass("crews__clamp");
    expect(within(runs).getAllByText("writer", { exact: false })).toHaveLength(3);
    fireEvent.click(within(runs).getByText("Broken"));
    expect(screen.getByText("LLM unavailable")).toHaveClass("crews__result--error");
    fireEvent.click(within(runs).getByText("Smoke"));
    expect(screen.getByText("OK")).toHaveClass("crews__result");
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
    expect(within(screen.getByRole("list", { name: "Runs" })).getByText("Running")).toBeInTheDocument();
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
    expect(screen.getByText("Completed")).toBeInTheDocument();
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
    fireEvent.click(screen.getByText("Smoke"));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("dialog", { name: "Stop this run?" });
    const confirm = within(dialog).getByRole("button", { name: "Stop Run" });
    expect(crew.stopCrew).not.toHaveBeenCalled();
    await act(async () => { fireEvent.click(confirm); });
    expect(crew.stopCrew).toHaveBeenCalledWith("r1");
    expect(within(screen.getByRole("list", { name: "Runs" })).getByText("Stopped")).toBeInTheDocument();
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
  const tools = (_root: HTMLElement) => within(screen.getByRole("region", { name: "Tools" }));
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
    expect(screen.queryByRole("region", { name: "Tools" })).toBeNull();
  });
  it("never renders keys, env values or URLs", () => {
    render(<CrewsField data={snap({ tools: [{ name: "kb_query", allowed: true }] })} />);
    expect(screen.getByRole("region", { name: "Tools" }).textContent).not.toMatch(/http|CREW_|SECRET|=/);
  });
});

