import { describe, it, expect, vi, beforeEach } from "vitest";

// Custom jobs: Activity and lastResult never carry raw errors, and a failed Telegram delivery
// is reported as a failure (it used to read "LLM complete").
const m = vi.hoisted(() => ({
  llm: vi.fn(async () => ({ content: "A meaningful update about the portfolio and goals today.", provider: "ollama", model: "gemma4" })),
  send: vi.fn(async () => undefined),
  bus: { register: vi.fn(() => "t1"), complete: vi.fn(), fail: vi.fn(), update: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "chat_with_router") return m.llm();
    if (cmd === "send_telegram_message") return m.send();
    return null;
  }),
}));
vi.mock("@/lib/api", () => ({ getConfig: vi.fn(async () => ({})) }));
vi.mock("@/lib/llm-router", () => ({ buildRouterConfig: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));
vi.mock("./deliberation", () => ({ deliberate: vi.fn() }));

import { runJob, type CustomJob } from "./customJobs";

const job = { id: "j1", name: "Check", prompt: "p", schedule: "daily", parsedSchedule: null, enabled: true, silent: false, lastRun: 0, lastResult: null, createdAt: 0 } as unknown as CustomJob;

describe("custom job outcomes", () => {
  beforeEach(() => { vi.clearAllMocks(); vi.spyOn(console, "error").mockImplementation(() => {}); });

  it("delivered output completes as Delivered", async () => {
    const r = await runJob(job);
    expect(r.delivered).toBe(true);
    expect(m.bus.complete).toHaveBeenCalledWith("t1", "Delivered");
  });

  it("a failed delivery is a failure with a safe reason, not 'LLM complete'", async () => {
    m.send.mockRejectedValueOnce(new Error("Telegram send refused: NEXUS_OUTBOUND_DISABLED=1"));
    const r = await runJob(job);
    expect(r.delivered).toBe(false);
    expect(m.bus.fail).toHaveBeenCalledWith("t1", "Not delivered: delivery paused (NEXUS_OUTBOUND_DISABLED)");
    expect(m.bus.complete).not.toHaveBeenCalled();
  });

  it("an LLM error never echoes the raw message (URLs, provider text)", async () => {
    m.llm.mockRejectedValueOnce(new Error("POST https://secret-host.example/v1 failed: token=abc"));
    const r = await runJob(job);
    expect(m.bus.fail).toHaveBeenCalledWith("t1", "generation failed");
    expect(r.output).toBe("Error: generation failed");
    expect(JSON.stringify(m.bus.fail.mock.calls)).not.toContain("secret-host");
  });
});
