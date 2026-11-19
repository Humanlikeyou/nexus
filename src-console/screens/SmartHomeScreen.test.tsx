import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockHaBackend, mockEntity, setHaBackend } from "../lib/homeAssistant";
import { SmartHomeField } from "./SmartHomeScreen";

let restore: (() => void) | null = null;
const use = (over: Parameters<typeof createMockHaBackend>[0] = {}) => { const b = createMockHaBackend(over); restore = setHaBackend(b); return b; };
afterEach(() => { restore?.(); restore = null; });

describe("Smart home", () => {
  it("lists devices with toggles that call ha_call_service, and applies live updates", async () => {
    const callService = vi.fn(async () => {});
    const b = use({ callService });
    render(<SmartHomeField />);
    const lamp = await screen.findByRole("switch", { name: "Desk lamp" });
    expect(lamp).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText("Sun")).toBeNull();
    expect(screen.getByText("21.5 °C")).toBeInTheDocument();
    fireEvent.click(lamp);
    await waitFor(() => expect(callService).toHaveBeenCalledWith("light", "turn_off", "light.desk_lamp"));
    act(() => b.emit({ entityId: "switch.kettle", newState: mockEntity({ entityId: "switch.kettle", friendlyName: "Kettle", state: "on" }), oldState: null, timeFired: null }));
    expect(screen.getByRole("switch", { name: "Kettle" })).toHaveAttribute("aria-checked", "true");
    act(() => b.emit({ entityId: "switch.kettle", newState: null, oldState: null, timeFired: null }));
    expect(screen.queryByRole("switch", { name: "Kettle" })).toBeNull();
    act(() => b.emitLive("connected"));
    expect(screen.getByLabelText("Live updates: Live")).toBeInTheDocument();
  });

  it.each([
    ["not_allowed: lock.unlock refused", "Not allowed from Nexus without confirmation."],
    ["needs_approval: confirm required", "Not allowed from Nexus without confirmation."],
    ["declined: user cancelled", "Not changed."],
    ["upstream_error: 500", "Couldn't turn off Desk lamp."],
  ])("toggle refusal %s: fixed copy next to the row, switch reverted", async (err, copy) => {
    use({ callService: async () => { throw err; } });
    render(<SmartHomeField />);
    const lamp = await screen.findByRole("switch", { name: "Desk lamp" });
    fireEvent.click(lamp);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(copy);
    expect(alert).not.toHaveTextContent(err);
    expect(alert.closest("li")).toBe(lamp.closest("li"));
    expect(lamp).toHaveAttribute("aria-checked", "true");
    expect(lamp).toBeEnabled();
  });

  it("shows the empty state", async () => {
    use({ states: async () => [] });
    render(<SmartHomeField />);
    expect(await screen.findByText("No devices yet.")).toBeInTheDocument();
  });

  it.each([
    ["Home Assistant is not configured (check HA_URL and HA_TOKEN).", "Home Assistant isn't set up"],
    ["Home Assistant is not reachable.", "Home Assistant isn't reachable"],
    ["Home Assistant rejected the token.", "Home Assistant rejected the token"],
  ])("inactive setup steps for %s", async (err, title) => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    use({ states: async () => { throw err; } });
    render(<SmartHomeField />);
    const region = await screen.findByRole("region", { name: "Home Assistant setup" });
    expect(region).toHaveClass("subsystem-inactive");
    expect(region).toHaveTextContent(title);
    expect(region).not.toHaveTextContent(err);
  });

  it("live authFailed shows token setup guidance", async () => {
    const b = use();
    render(<SmartHomeField />);
    await screen.findByRole("switch", { name: "Desk lamp" });
    act(() => b.emitLive("authFailed"));
    expect(screen.getByRole("region", { name: "Home Assistant setup" })).toHaveTextContent("rejected the token");
  });

  it("never starts/stops the container on mount, rerender or live events; once per click; no double-click", async () => {
    let finish: (v: { running: boolean; status: string }) => void = () => {};
    const containerStart = vi.fn(() => new Promise<{ running: boolean; status: string }>((r) => { finish = r; }));
    const containerStop = vi.fn(async () => ({ running: false, status: "Exited (0)" }));
    const containerStatus = vi.fn(async () => ({ running: false, status: "Exited (0) 1 hour ago" }));
    const b = use({ containerStart, containerStop, containerStatus });
    const { rerender } = render(<SmartHomeField />);
    await screen.findByLabelText("Home Assistant container: Down");
    rerender(<SmartHomeField />);
    act(() => { b.emitLive("disconnected"); b.emitLive("connecting"); });
    expect(containerStart).not.toHaveBeenCalled();
    expect(containerStop).not.toHaveBeenCalled();

    const start = screen.getByRole("button", { name: "Start" });
    fireEvent.click(start);
    fireEvent.click(start);
    expect(containerStart).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.getByRole("region", { name: "Container" })).toHaveAttribute("aria-busy", "true"));
    expect(screen.getByText(/up to 2 minutes/)).toBeInTheDocument();
    await act(async () => finish({ running: true, status: "Up 1 second" }));
    expect(await screen.findByLabelText("Home Assistant container: Up")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(containerStop).not.toHaveBeenCalled(); // asks first
    fireEvent.click(within(screen.getByRole("dialog", { name: "Stop Home Assistant?" })).getByRole("button", { name: "Stop" }));
    await waitFor(() => expect(containerStop).toHaveBeenCalledTimes(1));
    expect(containerStart).toHaveBeenCalledTimes(1);
  });

  describe("climate", () => {
    const hall = () => mockEntity({ entityId: "climate.hall", friendlyName: "Hall", state: "heat", attributes: { temperature: 21, hvac_modes: ["off", "heat", "eco"], min_temp: 7, max_temp: 35, target_temp_step: 0.5 } });

    it("stepper and typed value send set_temperature with only temperature, clamped", async () => {
      const callService = vi.fn(async () => {});
      use({ states: async () => [hall()], callService });
      render(<SmartHomeField />);
      fireEvent.click(await screen.findByRole("button", { name: "Raise Hall temperature" }));
      await waitFor(() => expect(callService).toHaveBeenLastCalledWith("climate", "set_temperature", "climate.hall", { temperature: 21.5 }));
      const input = screen.getByRole("spinbutton", { name: "Hall target temperature" });
      await waitFor(() => expect(input).toHaveValue(21.5));
      fireEvent.change(input, { target: { value: "99" } });
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(callService).toHaveBeenLastCalledWith("climate", "set_temperature", "climate.hall", { temperature: 35 }));
      await waitFor(() => expect(input).toHaveValue(35));
      expect(screen.getByRole("button", { name: "Raise Hall temperature" })).toBeDisabled();
    });

    it("mode select offers the intersection and sends only hvacMode", async () => {
      const callService = vi.fn(async () => {});
      use({ states: async () => [hall()], callService });
      render(<SmartHomeField />);
      const select = await screen.findByRole("combobox", { name: "Hall mode" });
      expect([...(select as HTMLSelectElement).options].map((o) => o.value)).toEqual(["off", "heat"]);
      fireEvent.change(select, { target: { value: "off" } });
      await waitFor(() => expect(callService).toHaveBeenCalledWith("climate", "set_hvac_mode", "climate.hall", { hvacMode: "off" }));
      await waitFor(() => expect(select).toHaveValue("off"));
    });

    it("refusal shows fixed copy and reverts the setpoint", async () => {
      use({ states: async () => [hall()], callService: async () => { throw "declined: The Home Assistant action was not approved."; } });
      render(<SmartHomeField />);
      fireEvent.click(await screen.findByRole("button", { name: "Lower Hall temperature" }));
      expect(await screen.findByRole("alert")).toHaveTextContent("Not changed.");
      await waitFor(() => expect(screen.getByRole("spinbutton", { name: "Hall target temperature" })).toHaveValue(21));
    });
  });
});
