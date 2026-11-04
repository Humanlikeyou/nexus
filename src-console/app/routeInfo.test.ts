import { describe, expect, it } from "vitest";
import type { TerrenceInteraction } from "../types";
import { routeInfoFrom } from "./routeInfo";
import { messagesFromInteractions } from "./conversation";

const base = (over: Record<string, unknown> = {}) => ({
  id: "i1", conversationId: "c", hermesSessionId: null, runtime: "hermes", model: "gemma4-12b-uncensored", provider: "custom",
  status: "succeeded", userMessage: "/deep plan", assistantMessage: "Plan.", error: null, startedAt: "2026-11-04T05:00:00Z",
  completedAt: "2026-11-04T05:00:30Z", durationMs: 30000, contextItemIds: [], actionId: null, runtimeProfile: "p", ...over,
}) as unknown as TerrenceInteraction;

describe("routeInfoFrom", () => {
  it("prefers the router's route + served model", () => {
    expect(routeInfoFrom(base({ route: { tier: "deep", model: "qwen3-32b", reason: "slash_deep" }, tier: { tier: "fast", reason: "", source: "router_log" } })))
      .toEqual({ tier: { tier: "deep", reason: "slash_deep", source: "router_reply" }, model: "qwen3-32b" });
    expect(routeInfoFrom(base({ routeTier: "light", servedModel: "gemma4-4b" }))).toMatchObject({ tier: { tier: "fast" }, model: "gemma4-4b" });
  });
  it("Rust's confirmed shape: route { tier } with model and reason optional", () => {
    expect(routeInfoFrom(base({ route: { tier: "fast" } }))).toEqual({ tier: { tier: "fast", reason: "", source: "router_reply" }, model: null });
  });
  it("claude keeps its own tier; JSON model fallback", () => {
    expect(routeInfoFrom(base({ route: { tier: "claude" }, routerModel: "claude-sonnet" }))).toMatchObject({ tier: { tier: "claude" }, model: "claude-sonnet" });
  });
  it("falls back to Rust's tier; never uses Hermes's configured model", () => {
    expect(routeInfoFrom(base({ tier: { tier: "deep", reason: "r", source: "router_log" } }))).toEqual({ tier: { tier: "deep", reason: "r", source: "router_log" }, model: null });
    expect(routeInfoFrom(base({ route: { tier: "weird" } }))).toEqual({ tier: null, model: null });
    expect(routeInfoFrom(base())).toEqual({ tier: null, model: null });
  });
  it("messages carry the tier badge and served model", () => {
    const [, a] = messagesFromInteractions([base({ route: { tier: "deep", model: "qwen3-32b" } })]);
    expect(a).toMatchObject({ tier: { tier: "deep" }, servedModel: "qwen3-32b", model: "gemma4-12b-uncensored" });
  });
});
