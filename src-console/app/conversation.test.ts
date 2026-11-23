import { describe, expect, it } from "vitest";
import type { TerrenceInteraction } from "../types";
import { messagesFromInteractions } from "./conversation";

const failed = (id: string, error: string) => ({
  id, conversationId: "c", hermesSessionId: null, runtime: "hermes", model: "m", provider: "p",
  status: "failed", userMessage: "hi", assistantMessage: null, error, startedAt: "2026-11-04T05:00:00Z",
  completedAt: "2026-11-04T05:00:01Z", durationMs: 1, contextItemIds: [], actionId: null, runtimeProfile: "p",
}) as unknown as TerrenceInteraction;

describe("messagesFromInteractions failures", () => {
  it("does not render retry signals saved by older builds", () => {
    const out = messagesFromInteractions([
      failed("a", "model_loading: retry_after=10"),
      failed("b", "Hermes is busy with another request. Try again in a moment."),
    ]);
    expect(out.map((m) => m.role)).toEqual(["user", "user"]);
  });

  it("still renders a real failure", () => {
    const out = messagesFromInteractions([failed("c", "Hermes could not complete the request.")]);
    expect(out).toHaveLength(2);
    expect(out[1]).toMatchObject({ role: "assistant", status: "failed", content: "Hermes could not complete the request." });
  });
});
