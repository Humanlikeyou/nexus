import { describe, it, expect } from "vitest";
import { buildRouterConfig } from "./llm-router";

// Compute mode "amara" (2026-10-31): the Amara Router at 127.0.0.1:8787 picks fast/deep.
describe("buildRouterConfig defaults", () => {
  it("defaults to the Amara Router", () => {
    expect(buildRouterConfig({})).toMatchObject({ mode: "amara", router_url: null });
  });
  it("passes a configured router URL through", () => {
    expect(buildRouterConfig({ amara_router_url: "http://127.0.0.1:9000" }).router_url).toBe("http://127.0.0.1:9000");
  });
  it("maps a saved retired \"gateway\" compute_mode to local", () => {
    expect(buildRouterConfig({ compute_mode: "gateway" }).mode).toBe("local");
  });
  it("keeps explicit settings", () => {
    expect(buildRouterConfig({ compute_mode: "hybrid", llm_ollama_model: "x" })).toMatchObject({ mode: "hybrid", ollama_model: "x" });
  });
});

// Lead decision 2026-10-29: "OpenClaw not installed" stays healthy because background jobs
// run on local gemma4. Guard: no background LLM call can be routed to the OpenClaw gateway.
describe("background LLM jobs never route to OpenClaw", () => {
  it("every chat_with_router call site uses buildRouterConfig and no forced provider", async () => {
    const { readFileSync, readdirSync } = await import("node:fs");
    const { join } = await import("node:path");
    const dir = join(__dirname, "../terrance/system");
    const files = readdirSync(dir).filter((f) => f.endsWith(".ts") && !f.includes(".test."));
    let sites = 0;
    for (const f of files) {
      const src = readFileSync(join(dir, f), "utf8");
      const calls = src.split('invoke<').slice(1).filter((c) => c.includes('"chat_with_router"') && c.indexOf('"chat_with_router"') < 80);
      for (const call of calls) {
        sites++;
        const body = call.slice(0, 600);
        expect(body, f).toMatch(/forceProvider:\s*undefined/);
        expect(body, f).not.toMatch(/"gateway"/);
      }
      if (calls.length) expect(src, f).toContain("buildRouterConfig(");
      expect(src, f).not.toMatch(/compute_mode:\s*"gateway"|mode:\s*"gateway"/);
    }
    expect(sites).toBeGreaterThanOrEqual(10);
  });
  it("the live config shape (compute_mode local) stays local (router, no cloud)", () => {
    expect(buildRouterConfig({ compute_mode: "local", llm_ollama_model: "gemma4:latest" })).toMatchObject({ mode: "local", ollama_model: "gemma4:latest" });
    expect(buildRouterConfig({ compute_mode: null })).toMatchObject({ mode: "amara" });
  });
});
