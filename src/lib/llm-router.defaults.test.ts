import { describe, it, expect } from "vitest";
import { buildRouterConfig } from "./llm-router";

// Standing rule (2026-10-29): Nexus uses local Ollama only, gemma4 primary.
describe("buildRouterConfig defaults", () => {
  it("defaults to local Ollama with gemma4:latest", () => {
    expect(buildRouterConfig({})).toMatchObject({ mode: "local", ollama_model: "gemma4:latest" });
  });
  it("keeps explicit settings", () => {
    expect(buildRouterConfig({ compute_mode: "hybrid", llm_ollama_model: "x" })).toMatchObject({ mode: "hybrid", ollama_model: "x" });
  });
});
