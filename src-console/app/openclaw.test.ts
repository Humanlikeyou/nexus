import { describe, expect, it } from "vitest";
import { openClawBadge } from "./openclaw";

const base = { online: false, model: "unknown", gateway_url: "", last_heartbeat: null };

describe("openClawBadge", () => {
  it("maps the three states", () => {
    expect(openClawBadge({ ...base, status: "online", online: true, model: "anthropic/claude" })).toEqual({ tone: "success", label: "Online", subtitle: "anthropic/claude" });
    expect(openClawBadge({ ...base, status: "online", online: true }).subtitle).toBe("Gateway reachable");
    expect(openClawBadge({ ...base, status: "configured" })).toMatchObject({ tone: "warning", label: "No heartbeat" });
    expect(openClawBadge({ ...base, status: "offline" })).toMatchObject({ tone: "neutral", label: "Not connected" });
    expect(openClawBadge({ ...base, status: "not_installed" })).toMatchObject({ tone: "neutral", label: "Not installed" });
  });
  it("falls back to `online` when status is missing", () => {
    expect(openClawBadge({ ...base, online: true }).label).toBe("Online");
    expect(openClawBadge(base).label).toBe("Not connected");
  });
});
