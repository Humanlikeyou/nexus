import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import {
  applyStateChange, classifyHaError, containerErrorCopy, createMockHaBackend, haErrorCopy, homeReplyIds, inactiveKindFor, isServiceRefusal, rememberHomeReply, isToggleable, mockEntity, preRouteHome, setHaBackend, sortDevices, stateLabel, tauriHaBackend, validServiceCall,
} from "./homeAssistant";

afterEach(() => invokeMock.mockReset());

describe("Home Assistant wrapper", () => {
  it("invokes the confirmed commands with camelCase args", async () => {
    invokeMock.mockResolvedValue(undefined);
    await tauriHaBackend.callService("light", "turn_on", "light.desk_lamp");
    expect(invokeMock).toHaveBeenCalledWith("ha_call_service", { domain: "light", service: "turn_on", entityId: "light.desk_lamp" });
    await tauriHaBackend.conversation("turn on the lamp");
    expect(invokeMock).toHaveBeenCalledWith("ha_conversation", { text: "turn on the lamp" });
  });

  it("validates service calls client-side before invoking", async () => {
    expect(validServiceCall("light", "turn_on", "light.desk_lamp")).toBe(true);
    for (const [d, s, e] of [["Light", "turn_on", "light.x"], ["light", "turn-on", "light.x"], ["light", "turn_on", "lightx"], ["light", "turn_on", "light.x/../y"], ["a".repeat(65), "s", "a.b"]]) {
      expect(validServiceCall(d, s, e)).toBe(false);
    }
    await expect(tauriHaBackend.callService("light", "turn_on", "../etc")).rejects.toMatch(/Invalid/);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it.each([
    ["not_configured", "Home Assistant isn't set up.", "not_configured"],
    ["bad_url", "Home Assistant's address must be on this Mac (127.0.0.1).", "not_configured"],
    ["auth_rejected", "Home Assistant rejected the token.", "auth"],
    ["unreachable", "Home Assistant isn't reachable.", "unreachable"],
    ["docker_unavailable", "Docker isn't available. Open Docker Desktop, then try again.", null],
    ["already_running", "A container action is already running.", null],
    ["timed_out", "That took too long. Try again.", null],
    ["too_large", "That's too long for Home Assistant.", null],
    ["not_allowed", "Not allowed from Nexus without confirmation.", null],
    ["needs_approval", "Not allowed from Nexus without confirmation.", null],
    ["declined", "Not changed.", null],
    ["domain_mismatch", "That service doesn't match this device.", null],
    ["invalid_input", "That request isn't valid.", null],
    ["upstream_error", "Home Assistant returned an error.", null],
  ] as const)("kind %s: parsed from the prefix, fixed copy, never the raw message", (kind, copy, inactive) => {
    const raw = `${kind}: secret detail /Users/x 1.2.3.4`;
    expect(classifyHaError(raw)).toBe(kind);
    expect(haErrorCopy(raw)).toBe(copy);
    expect(haErrorCopy(raw)).not.toContain("secret");
    expect(inactiveKindFor(kind)).toBe(inactive);
    expect(isServiceRefusal(kind)).toBe(kind === "not_allowed" || kind === "needs_approval" || kind === "declined");
  });

  it("falls back to the old substrings; unknown prefixes don't count", () => {
    expect(classifyHaError("Home Assistant is not configured (check HA_URL and HA_TOKEN).")).toBe("not_configured");
    expect(classifyHaError("HA_URL must be a loopback http address.")).toBe("bad_url");
    expect(classifyHaError("Home Assistant rejected the token.")).toBe("auth_rejected");
    expect(classifyHaError("Home Assistant is not reachable.")).toBe("unreachable");
    expect(classifyHaError("Docker is not available.")).toBe("docker_unavailable");
    expect(classifyHaError("Command ha_states not found")).toBe("backend_missing");
    expect(classifyHaError("weird: thing")).toBe("unknown");
    expect(containerErrorCopy("docker_unavailable: x")).toMatch(/Docker Desktop/);
    expect(containerErrorCopy("upstream_error: x")).toBe("The container command failed.");
  });

  it("remembers Home reply ids (ids only, capped)", () => {
    window.localStorage.clear();
    rememberHomeReply("i1");
    rememberHomeReply("i2");
    expect([...homeReplyIds()]).toEqual(["i1", "i2"]);
    window.localStorage.setItem("nexus.ha.homeReplyIds", "not json");
    expect(homeReplyIds().size).toBe(0);
  });

  it("filters bookkeeping entities, sorts, and labels states", () => {
    const list = sortDevices([mockEntity({ entityId: "switch.b", friendlyName: "B" }), mockEntity({ entityId: "sun.sun" }), mockEntity({ entityId: "light.a", state: "on" })]);
    expect(list.map((e) => e.entityId)).toEqual(["light.a", "switch.b"]);
    expect(isToggleable(list[0])).toBe(true);
    expect(isToggleable(mockEntity({ entityId: "light.x", state: "unavailable" }))).toBe(false);
    expect(stateLabel(mockEntity({ entityId: "sensor.t", state: "21.5", attributes: { unit_of_measurement: "°C" } }))).toBe("21.5 °C");
  });

  it("applies live state_changed in place; null newState removes", () => {
    const list = [mockEntity({ entityId: "light.a", state: "off" }), mockEntity({ entityId: "switch.b" })];
    const on = applyStateChange(list, { entityId: "light.a", newState: { ...list[0], state: "on" }, oldState: list[0], timeFired: null });
    expect(on.find((e) => e.entityId === "light.a")?.state).toBe("on");
    expect(applyStateChange(on, { entityId: "switch.b", newState: null, oldState: list[1], timeFired: null }).map((e) => e.entityId)).toEqual(["light.a"]);
  });

  it("pre-route: match handled; noMatch, errors and long text fall through", async () => {
    const restore = setHaBackend(createMockHaBackend({ conversation: async () => ({ kind: "match", speech: "Turned on the lamp." }) }));
    expect(await preRouteHome("turn on the lamp")).toEqual({ handled: true, speech: "Turned on the lamp." });
    expect(await preRouteHome("x".repeat(1001))).toEqual({ handled: false });
    restore();
    const r2 = setHaBackend(createMockHaBackend());
    expect(await preRouteHome("what's 2+2")).toEqual({ handled: false });
    r2();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const r3 = setHaBackend(createMockHaBackend({ conversation: async () => { throw "Home Assistant is not reachable."; } }));
    expect(await preRouteHome("lamp")).toEqual({ handled: false });
    expect(warn.mock.calls.flat().join(" ")).not.toMatch(/reachable\./);
    r3(); warn.mockRestore();
  });
});
