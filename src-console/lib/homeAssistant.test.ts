import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (invokeMock as (...x: unknown[]) => unknown)(...a); } }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import {
  applyStateChange, classifyHaError, containerErrorCopy, createMockHaBackend, haErrorCopy, inactiveKindFor, isServiceRefusal, dropLegacyHomeReplyIds, isToggleable, mockEntity, preRouteHome, setHaBackend, sortDevices, stateLabel, tauriHaBackend, validServiceCall, validServiceData, hvacModesFor, climateRange, HA_BAD_DATA, HA_ERROR_KINDS, looksLikeHomeCommand, isHaFailureSpeech, homeEntityNamesForTurn, rememberEntityNames,
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
    ["too_large", "That request was too large.", null],
    ["not_allowed", "Not allowed from Nexus without confirmation.", null],
    ["needs_approval", "Not allowed from Nexus without confirmation.", null],
    ["declined", "Not changed.", null],
    ["domain_mismatch", "That service doesn't match this device.", null],
    ["invalid_input", "That value isn't valid for this device.", null],
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

  it("drops the legacy Home reply id key", () => {
    window.localStorage.setItem("nexus.ha.homeReplyIds", "[\"i1\"]");
    dropLegacyHomeReplyIds();
    expect(window.localStorage.getItem("nexus.ha.homeReplyIds")).toBeNull();
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

  describe("climate data contract", () => {
    it("validServiceData allows exactly one field for the two climate services and nothing elsewhere", () => {
      expect(validServiceData("climate", "set_temperature", { temperature: 21.5 })).toBe(true);
      expect(validServiceData("climate", "set_temperature", { temperature: 5 })).toBe(true);
      expect(validServiceData("climate", "set_temperature", { temperature: 95 })).toBe(true);
      expect(validServiceData("climate", "set_hvac_mode", { hvacMode: "heat_cool" })).toBe(true);
      expect(validServiceData("light", "turn_on", undefined)).toBe(true);
      for (const t of [4.9, 95.1, NaN, Infinity]) expect(validServiceData("climate", "set_temperature", { temperature: t })).toBe(false);
      expect(validServiceData("climate", "set_temperature", undefined)).toBe(false);
      expect(validServiceData("climate", "set_temperature", {})).toBe(false);
      expect(validServiceData("climate", "set_temperature", { temperature: 20, hvacMode: "heat" })).toBe(false);
      expect(validServiceData("climate", "set_hvac_mode", { hvacMode: "turbo" as never })).toBe(false);
      expect(validServiceData("climate", "set_hvac_mode", { temperature: 20 })).toBe(false);
      expect(validServiceData("climate", "turn_on", { temperature: 20 })).toBe(false);
      expect(validServiceData("light", "turn_on", { temperature: 20 })).toBe(false);
      expect(validServiceData("climate", "set_temperature", { temperature: 20, extra: 1 } as never)).toBe(false);
    });

    it("sends data only for climate services and rejects bad data without invoking", async () => {
      invokeMock.mockResolvedValue(undefined);
      await tauriHaBackend.callService("climate", "set_temperature", "climate.hall", { temperature: 21 });
      expect(invokeMock).toHaveBeenLastCalledWith("ha_call_service", { domain: "climate", service: "set_temperature", entityId: "climate.hall", data: { temperature: 21 } });
      await tauriHaBackend.callService("climate", "set_hvac_mode", "climate.hall", { hvacMode: "cool" });
      expect(invokeMock).toHaveBeenLastCalledWith("ha_call_service", { domain: "climate", service: "set_hvac_mode", entityId: "climate.hall", data: { hvacMode: "cool" } });
      await tauriHaBackend.callService("climate", "turn_off", "climate.hall");
      expect(invokeMock.mock.lastCall?.[1]).not.toHaveProperty("data");
      invokeMock.mockClear();
      await expect(tauriHaBackend.callService("light", "turn_on", "light.x", { temperature: 20 })).rejects.toBe(HA_BAD_DATA);
      await expect(tauriHaBackend.callService("climate", "set_temperature", "climate.hall", { temperature: 120 })).rejects.toBe(HA_BAD_DATA);
      expect(invokeMock).not.toHaveBeenCalled();
      expect(haErrorCopy(HA_BAD_DATA)).toBe("That value isn't valid for this device.");
    });

    it("hvac options intersect the entity's hvac_modes; range clamps to 5–95", () => {
      const e = mockEntity({ entityId: "climate.hall", state: "heat", attributes: { hvac_modes: ["off", "heat", "eco", "auto"], min_temp: 0, max_temp: 120, target_temp_step: 1 } });
      expect(hvacModesFor(e)).toEqual(["off", "heat", "auto"]);
      expect(hvacModesFor(mockEntity({ entityId: "climate.x" }))).toHaveLength(7);
      expect(climateRange(e)).toEqual({ min: 5, max: 95, step: 1 });
      expect(climateRange(mockEntity({ entityId: "climate.x", attributes: { min_temp: 16, max_temp: 30 } }))).toEqual({ min: 16, max: 30, step: 0.5 });
    });

    it("maps the exact Rust refusal strings and keeps 14 kinds", () => {
      expect(HA_ERROR_KINDS).toHaveLength(14);
      expect(haErrorCopy("not_allowed: That Home Assistant action is not allowed.")).toBe("Not allowed from Nexus without confirmation.");
      expect(haErrorCopy("declined: The Home Assistant action was not approved.")).toBe("Not changed.");
      expect(classifyHaError("domain_mismatch: The entity does not belong to the service's domain.")).toBe("domain_mismatch");
      expect(classifyHaError("invalid_input: Invalid or missing data for this Home Assistant service.")).toBe("invalid_input");
    });
  });

  describe("pre-route gate", () => {
    it.each(["turn on the desk lamp", "Switch off the kettle", "set the thermostat to 21", "dim the lights", "Are the lights on", "start the vacuum"])("device command: %s", (t) => {
      expect(looksLikeHomeCommand(t, ["desk lamp", "kettle"])).toBe(true);
    });
    it.each(["please lock the front door", "unlock the front door", "can you open the blinds", "is the garage door open?", "open the garage", "set the alarm", "close the curtains", "raise the shutters", "open the cover"])("security and cover commands are never auto-run: %s", (t) => {
      expect(looksLikeHomeCommand(t, ["front door", "garage"])).toBe(false);
    });
    it.each(["what is 2+2", "2 + 2", "what's 12 * 4", "hi", "open safari", "set a reminder for 5pm", "stop", "what is the capital of France?", "turn on the lights ".repeat(10), "turn on the light\nand write me an essay about lamps"])("skips: %s", (t) => {
      expect(looksLikeHomeCommand(t, [])).toBe(false);
    });
    it("known friendly names count as devices", () => {
      expect(looksLikeHomeCommand("turn on Bubbles", ["bubbles"])).toBe(true);
      expect(looksLikeHomeCommand("turn on Bubbles", [])).toBe(false);
    });
    it("homeEntityNamesForTurn caps names like Rust (200 names, 64 chars)", () => {
      rememberEntityNames([
        ...Array.from({ length: 250 }, (_, i) => mockEntity({ entityId: `light.l${i}`, friendlyName: `lamp ${i}` })),
        mockEntity({ entityId: "light.long", friendlyName: "x".repeat(65) }),
      ]);
      const names = homeEntityNamesForTurn();
      expect(names).toHaveLength(200);
      expect(names.every((n) => n.length <= 64)).toBe(true);
      rememberEntityNames([]);
    });
    it("HA failure speech is detected", () => {
      expect(isHaFailureSpeech("Sorry, I am not aware of any device called 2+2")).toBe(true);
      expect(isHaFailureSpeech("Sorry, I couldn't understand that")).toBe(true);
      expect(isHaFailureSpeech("Turned on the desk lamp")).toBe(false);
    });
    it("a real device command still matches end to end", async () => {
      const conversation = vi.fn(async () => ({ kind: "match" as const, speech: "Turned off the light" }));
      const restore = setHaBackend(createMockHaBackend({ conversation }));
      expect(await preRouteHome("turn off the desk lamp")).toEqual({ handled: true, speech: "Turned off the light" });
      expect(await preRouteHome("what is 2+2")).toEqual({ handled: false });
      conversation.mockResolvedValueOnce({ kind: "match", speech: "Sorry, I couldn't understand that" });
      expect(await preRouteHome("turn off the desk lamp")).toEqual({ handled: false });
      expect(conversation).toHaveBeenCalledTimes(2);
      restore();
    });
  });
});
