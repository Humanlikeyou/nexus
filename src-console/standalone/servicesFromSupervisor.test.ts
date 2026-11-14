import { describe, expect, it } from "vitest";
import type { ReasonCode, ServiceId, ServiceState, ServiceStatus } from "../bindings.gen";
import { REASON_TEXT, eventsFromSupervisor, isSupStatus, serviceFromSupervisor, servicesFromSupervisor } from "./servicesFromSupervisor";
import { summaryFromServices, summaryFromSupervisor } from "./servicesSummary";

const row = (id: ServiceId, state: ServiceState, over: Partial<ServiceStatus> = {}): ServiceStatus => ({
  id, state, reasonCode: "ok", sinceMs: 1, checkedMs: 1_700_000_000_000, attempt: 0, maxAttempts: 3, owner: "supervisor", adopted: false,
  can: { restart: true, start: false, stop: false }, ...over,
});

describe("servicesFromSupervisor", () => {
  it("maps each state", () => {
    expect(serviceFromSupervisor(row("n8n", "unknown", { reasonCode: "not_checked" })).state).toBe("waiting");
    expect(serviceFromSupervisor(row("n8n", "starting", { reasonCode: "container_starting" })).state).toBe("waiting");
    expect(serviceFromSupervisor(row("n8n", "healthy")).state).toBe("healthy");
    expect(serviceFromSupervisor(row("n8n", "degraded", { reasonCode: "slow" })).state).toBe("degraded");
    expect(serviceFromSupervisor(row("n8n", "stopped", { reasonCode: "container_stopped" })).state).toBe("stopped");
  });
  it("needs_you is failed with fixed reason text", () => {
    const s = serviceFromSupervisor(row("crew-service", "needs_you", { reasonCode: "restart_failed", attempt: 3 }));
    expect(s.state).toBe("failed");
    expect(s.what).toBe(REASON_TEXT.restart_failed);
    expect(s.meta).toBe("Needs you");
  });
  it("restarting carries the real counters and the owner", () => {
    const s = serviceFromSupervisor(row("crew-service", "restarting", { reasonCode: "restarting", attempt: 2, maxAttempts: 3 }));
    expect(s).toMatchObject({ state: "restarting", restartAttempt: 2, restartMax: 3, reasonOwner: "supervisor", meta: "try 2 of 3" });
  });
  it("stopped by the user says so; owner is kept", () => {
    expect(serviceFromSupervisor(row("homeassistant", "stopped", { owner: "user", reasonCode: "container_stopped" }))).toMatchObject({ meta: "by you", reasonOwner: "user" });
  });
  it("separate shows as healthy with a fixed meta (no separate pill yet)", () => {
    const s = serviceFromSupervisor(row("open-webui", "separate"));
    expect(s.state).toBe("healthy");
    expect(s.meta).toBe("Separate app · watched only");
    expect(s.group).toBe("side");
  });
  it("health-reader and health-sync are built in", () => {
    expect(serviceFromSupervisor(row("health-reader", "healthy")).builtIn).toBe(true);
    expect(serviceFromSupervisor(row("health-sync", "unknown", { owner: "transition" }))).toMatchObject({ builtIn: true, reasonOwner: "transition" });
    expect(serviceFromSupervisor(row("docker", "healthy")).builtIn).toBeUndefined();
  });
  it("invents no numbers: meta and what contain digits only from real restart counters", () => {
    const states: ServiceState[] = ["unknown", "starting", "healthy", "degraded", "needs_you", "stopped", "separate"];
    for (const st of states) for (const code of Object.keys(REASON_TEXT) as ReasonCode[]) {
      const s = serviceFromSupervisor(row("postgres", st, { reasonCode: code, attempt: 0 }));
      expect(`${s.what} ${s.meta}`).not.toMatch(/\d/);
    }
  });
  it("one row per status, checked time from the newest check", () => {
    const o = servicesFromSupervisor([row("n8n", "healthy"), row("letta", "healthy", { checkedMs: 0 })]);
    expect(o.services.map((s) => s.id)).toEqual(["n8n", "letta"]);
    expect(o.checked).not.toBe("");
    expect(servicesFromSupervisor([]).checked).toBe("");
  });
  it("validates rows", () => {
    expect(isSupStatus(row("n8n", "healthy"))).toBe(true);
    expect(isSupStatus({ ...row("n8n", "healthy"), state: "bogus" })).toBe(false);
    expect(isSupStatus({ ...row("n8n", "healthy"), id: "router" })).toBe(false);
    expect(isSupStatus(null)).toBe(false);
  });
});

describe("events and summary", () => {
  const ev = (kind: "state_changed" | "gave_up" | "restart_requested" | "adopted", state: ServiceState, text: string) => ({ seq: 1, tsMs: 1_700_000_000_000, id: "n8n" as ServiceId, kind, state, reasonCode: "ok" as ReasonCode, text });
  it("maps events with fixed text and levels", () => {
    const out = eventsFromSupervisor([ev("adopted", "healthy", "Found it already running."), ev("state_changed", "degraded", "Slow to answer."), ev("gave_up", "needs_you", "Gave up after 3 tries.")]);
    expect(out.map((e) => e.level)).toEqual(["ok", "warn", "fail"]);
    expect(out.map((e) => e.text)).toEqual(["Found it already running.", "Slow to answer.", "Gave up after 3 tries."]);
    expect(out[0].at).not.toBe("");
  });
  it("summary uses the backend attention count; malformed is null", () => {
    expect(summaryFromSupervisor({ total: 15, attention: 2 })).toEqual({ attention: 2, updateReady: false });
    expect(summaryFromSupervisor({ attention: "2" })).toBeNull();
    expect(summaryFromSupervisor(undefined)).toBeNull();
    expect(summaryFromServices(servicesFromSupervisor([row("n8n", "needs_you"), row("letta", "healthy")]).services).attention).toBe(1);
  });
});
