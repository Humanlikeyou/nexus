import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../../bridge";
import type { SystemSnapshot } from "../../types";
import { ACTIVITY_ROUTE, CREWS_ROUTE, NEEDS_YOU_ROUTE, crewRunOf, crewRunRoute, tabOf } from "../../app/routes";
import { AgentOrbs, TodaysPulse } from "../Agents";
import { LiveHomeDevices } from "../LiveHomeDevices";
import { NowFeedView, buildNowRows } from "../NowFeed";
import { ActivityField } from "../../screens/ActivityScreen";
import { SmartHomeField } from "../../screens/SmartHomeScreen";
import { createMockHaBackend, mockEntity, setHaBackend } from "../../lib/homeAssistant";

const snap = (over: Partial<SystemSnapshot> = {}): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ...over });
const crewRun = { id: "r9", name: "smoke", status: "running", agents: [], started_at: null, task: null, result: null };

describe("Agents: rows, orbs and chips go somewhere (decision 0042)", () => {
  it("crew rows open the run page, waiting actions open Needs you, deliveries open Activity", () => {
    const data = snap({
      crew: { health: "online", runs: [crewRun as never, { ...crewRun, id: "r8", status: "awaiting_approval" } as never], tools: [] },
      actions: [{ id: "a1", toolId: "system.run", status: "awaiting_approval", title: "x", requestedAt: "2026-11-19T00:00:00Z" } as never],
      outbox: { ...INITIAL_SNAPSHOT.outbox, queue_depth: 2 },
    });
    const rows = buildNowRows(data, { backend: null, jobs: null });
    expect(rows.find((r) => r.key === "cr9")?.to).toEqual(crewRunRoute("r9"));
    expect(rows.find((r) => r.key === "aa1")?.to).toEqual(NEEDS_YOU_ROUTE);
    expect(rows.find((r) => r.key === "outbox")?.to).toEqual(ACTIVITY_ROUTE);
    const onOpen = vi.fn();
    render(<NowFeedView data={{ ...data, crew: { health: "online", runs: [crewRun as never], tools: [] } }} backend={null} jobs={null} now={Date.now()} onOpen={onOpen} />);
    fireEvent.click(screen.getByText("Crew: smoke"));
    expect(onOpen).toHaveBeenLastCalledWith({ id: "chat", view: "crews/run/r9" });
    fireEvent.click(screen.getByText("Delivery"));
    expect(onOpen).toHaveBeenLastCalledWith(ACTIVITY_ROUTE);
  });

  it("a run route stays on the Crews tab and round-trips its id", () => {
    expect(tabOf(crewRunRoute("r9"))).toBe("crews");
    expect(crewRunOf(crewRunRoute("r9"))).toBe("r9");
    expect(crewRunOf(CREWS_ROUTE)).toBeNull();
  });

  it("orbs: Crews and Deliveries open their pages; the others stay plain", () => {
    const onOpen = vi.fn();
    render(<AgentOrbs data={snap()} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("button", { name: /Open Crews/ }));
    expect(onOpen).toHaveBeenLastCalledWith(CREWS_ROUTE);
    // A native <button> inside the list item (axe: lists hold only list items), so Enter/Space work natively.
    const deliveries = screen.getByRole("button", { name: /Open Deliveries/ });
    expect(deliveries.tagName).toBe("BUTTON");
    expect(deliveries.closest("li")).not.toBeNull();
    fireEvent.click(deliveries);
    expect(onOpen).toHaveBeenLastCalledWith(ACTIVITY_ROUTE);
    expect(screen.queryByRole("button", { name: /Open Tools/ })).toBeNull();
  });

  it("without onOpen nothing is a button", () => {
    render(<AgentOrbs data={snap()} />);
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("today's pulse links to Activity", () => {
    const now = Date.now();
    const onOpen = vi.fn();
    render(<TodaysPulse now={now} onOpen={onOpen} data={snap({ activity: [{ id: "h1", name: "health_check", type: "health_check", status: "completed", startedAt: now - 1000, durationMs: 5 } as never] })} />);
    fireEvent.click(screen.getByRole("button", { name: /See activity/ }));
    expect(onOpen).toHaveBeenCalledWith(ACTIVITY_ROUTE);
  });

  it("an Activity row from a crew opens Crews", () => {
    const onOpen = vi.fn();
    const item = { id: "e1", name: "crew_run", type: "crew", status: "completed", startedAt: 1, completedAt: 2, durationMs: 1, detail: "", outcome: "" };
    render(<ActivityField data={snap({ activity: [item as never] })} onOpen={onOpen} />);
    fireEvent.click(screen.getByText("Crew run"));
    expect(onOpen).toHaveBeenCalledWith(CREWS_ROUTE);
  });
});

const dev = (o: object) => ({ entityId: "x.1", name: "N", deviceType: "light", room: "Living", state: "off", unit: null, system: false, controllable: true, services: ["turn_on", "turn_off"], brightnessPct: null, percentage: null, temperature: null, currentTemperature: null, minTemperature: null, maxTemperature: null, hvacModes: [], volume: null, levelService: null, level: null, levelMax: null, ...o });

describe("Smart home: one source of truth and every control completes", () => {
  const climate = dev({ entityId: "climate.hall", name: "Hall", deviceType: "climate", state: "heat", services: ["set_temperature", "set_hvac_mode"], temperature: 21, minTemperature: 7, maxTemperature: 35, hvacModes: ["off", "heat"] });
  it("climate goes through ha_device_set with a note and Undo", async () => {
    const set = vi.fn(async () => ({ id: "h1", undoable: true, message: "Hall set to 22." }));
    const undo = vi.fn(async () => true);
    render(<LiveHomeDevices calls={{ list: async () => ({ roomsAvailable: false, devices: [climate] }), set, undo } as never} />);
    fireEvent.click(await screen.findByRole("button", { name: "Raise Hall temperature" }));
    await waitFor(() => expect(set).toHaveBeenCalledWith("climate.hall", "set_temperature", 22, "typed"));
    expect(await screen.findByText(/Hall set to 22/)).toBeTruthy();
    fireEvent.change(screen.getByRole("combobox", { name: "Hall mode" }), { target: { value: "off" } });
    await waitFor(() => expect(set).toHaveBeenLastCalledWith("climate.hall", "set_hvac_mode", "off", "typed"));
    fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(undo).toHaveBeenCalledWith("h1"));
  });

  it("a failed control says so and offers no Undo", async () => {
    const set = vi.fn(async () => { throw "declined"; });
    render(<LiveHomeDevices calls={{ list: async () => ({ roomsAvailable: false, devices: [climate] }), set, undo: vi.fn() } as never} />);
    fireEvent.click(await screen.findByRole("button", { name: "Lower Hall temperature" }));
    expect(await screen.findByText(/Hall didn't change/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("re-reads the list when the refresh signal changes, and reports what it lists", async () => {
    const list = vi.fn(async () => ({ roomsAvailable: false, devices: [dev({ entityId: "light.a", name: "Lamp" })] }));
    const onListed = vi.fn();
    const { rerender } = render(<LiveHomeDevices calls={{ list, set: vi.fn(), undo: vi.fn() } as never} onListed={onListed} refreshSignal={0} />);
    await screen.findByText("Lamp");
    expect(onListed).toHaveBeenLastCalledWith(new Set(["light.a"]));
    rerender(<LiveHomeDevices calls={{ list, set: vi.fn(), undo: vi.fn() } as never} onListed={onListed} refreshSignal={1} />);
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
  });

  const states = () => [
    mockEntity({ entityId: "light.a", friendlyName: "Lamp", state: "off" }),
    mockEntity({ entityId: "input_boolean.guest", friendlyName: "Guest mode", state: "off" }),
  ];
  it("page: a device the live panel lists appears once (no grouped duplicate); the rest keep their row", async () => {
    const b = createMockHaBackend({ states: async () => states() });
    const restore = setHaBackend(b);
    const calls = { list: async () => ({ roomsAvailable: false, devices: [dev({ entityId: "light.a", name: "Lamp" })] }), set: vi.fn(), undo: vi.fn() };
    render(<SmartHomeField liveDevices homeCalls={calls as never} />);
    await screen.findByTestId("live-devices");
    await screen.findByText("Guest mode");
    expect(screen.getAllByText("Lamp")).toHaveLength(1);
    expect(screen.queryByRole("checkbox", { name: "Lamp" })).toBeNull();
    restore();
  });

  it("page: a state event for a listed device refreshes the live panel", async () => {
    const b = createMockHaBackend({ states: async () => states() });
    const restore = setHaBackend(b);
    const list = vi.fn(async () => ({ roomsAvailable: false, devices: [dev({ entityId: "light.a", name: "Lamp" })] }));
    render(<SmartHomeField liveDevices homeCalls={{ list, set: vi.fn(), undo: vi.fn() } as never} />);
    await screen.findByTestId("live-devices");
    const before = list.mock.calls.length;
    act(() => { b.emit({ entityId: "light.a", newState: mockEntity({ entityId: "light.a", friendlyName: "Lamp", state: "on" }) } as never); });
    await waitFor(() => expect(list.mock.calls.length).toBeGreaterThan(before));
    restore();
  });

  it("page: Stop asks first; Cancel changes nothing; confirming stops and says what it did", async () => {
    const containerStop = vi.fn(async () => ({ running: false, status: "Exited (0)" }));
    const restore = setHaBackend(createMockHaBackend({ states: async () => states(), containerStatus: async () => ({ running: true, status: "Up" }), containerStop }));
    render(<SmartHomeField />);
    await screen.findByLabelText("Home Assistant container: Up");
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("dialog", { name: "Stop Home Assistant?" });
    expect(dialog).toHaveTextContent(/can't read or control them until you start it again/);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(containerStop).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Stop Home Assistant?" })).getByRole("button", { name: "Stop" }));
    expect(await screen.findByText(/Home Assistant stopped/)).toBeTruthy();
    restore();
  });

  it("page: the token-rejected panel offers Home Assistant settings", async () => {
    const b = createMockHaBackend({ states: async () => states() });
    const restore = setHaBackend(b);
    const onOpenSettings = vi.fn();
    render(<SmartHomeField onOpenSettings={onOpenSettings} />);
    await screen.findByText("Guest mode");
    act(() => { b.emitLive("authFailed"); });
    fireEvent.click(await screen.findByRole("button", { name: "Open Home Assistant settings" }));
    expect(onOpenSettings).toHaveBeenCalled();
    restore();
  });

  it("legacy list (outside the app): toggle and scene Run now confirm; the scene says it can't be undone", async () => {
    const callService = vi.fn(async () => {});
    const restore = setHaBackend(createMockHaBackend({ states: async () => [mockEntity({ entityId: "input_boolean.guest", friendlyName: "Guest mode", state: "off" }), mockEntity({ entityId: "scene.evening", friendlyName: "Evening", state: "scening" })], callService }));
    render(<SmartHomeField />);
    fireEvent.click(await screen.findByRole("switch", { name: "Guest mode" }));
    expect(await screen.findByText("Turned on Guest mode.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Run Evening" }));
    expect(await screen.findByText(/Ran Evening\. A scene can't be undone/)).toBeTruthy();
    restore();
  });
});
