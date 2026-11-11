// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Hostile errors must never reach Activity (activityBus fail/complete/suppress/update) or a
// persisted result (custom job lastResult, research summary, auto-heal report, Telegram text).
// Every path below runs the real module with invoke() throwing the hostile error.
const FAKE = "FAKE_TOKEN_NOT_REAL";
const m = vi.hoisted(() => ({
  hostile: undefined as unknown,
  /** Commands that succeed instead of throwing, with their result. */
  ok: new Map<string, (args: Record<string, unknown>) => unknown>(),
  sent: [] as string[],
  stored: [] as unknown[],
  bus: {
    register: vi.fn((name: string) => name),
    complete: vi.fn(), fail: vi.fn(), suppress: vi.fn(), update: vi.fn(), prune: vi.fn(),
  },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    if (cmd === "send_telegram_message") m.sent.push(String(args.text));
    if (cmd === "store_session_log") m.stored.push(args.entry);
    const ok = m.ok.get(cmd);
    if (ok) return ok(args);
    throw m.hostile;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

import { jobFailureReason } from "./continuity";

const stackErr = (() => {
  const e = new Error(`boom ${FAKE}`);
  e.message += "\n    at Object.<anonymous> (/Users/thevisualdude/projects/nexus/src/x.ts:12:7)\n    at run (node:internal/process:1:1)";
  return e;
})();
const HOSTILE: [string, unknown][] = [
  ["URL with a query token", new Error(`fetch failed: https://example.invalid/hook?token=${FAKE}`)],
  ["Bearer header", new Error(`401 Unauthorized (Authorization: Bearer ${FAKE})`)],
  ["stack trace", stackErr],
  ["5 KB message", new Error(`${"x".repeat(5_000)} ${FAKE}`)],
  ["JSON with api_key", new Error(JSON.stringify({ error: "bad request", api_key: FAKE }))],
  ["file path under /Users/", new Error("ENOENT: open '/Users/thevisualdude/Library/Application Support/com.taisun.nexus/x.json'")],
  ["thrown string", `raw ${FAKE} /Users/thevisualdude/secret.txt`],
  ["thrown object", { code: 500, detail: `api_key=${FAKE}`, path: "/Users/thevisualdude" }],
  ["thrown undefined", undefined],
];

function assertSafe(text: unknown, where: string) {
  const s = String(text ?? "");
  expect(s, where).not.toContain(FAKE);
  expect(s, where).not.toContain("Bearer");
  expect(s, where).not.toContain("api_key");
  expect(s, where).not.toContain("/Users/");
  expect(s, where).not.toMatch(/(^|\n)\s*at \S|\(\S+:\d+:\d+\)/);
  expect(s.length, `${where}: ${s.length} chars`).toBeLessThanOrEqual(160);
}

/** Every detail text that reached Activity in this case. */
function activityTexts(): string[] {
  return [
    ...m.bus.fail.mock.calls.map((c) => c[1]),
    ...m.bus.complete.mock.calls.map((c) => c[1]),
    ...m.bus.suppress.mock.calls.map((c) => c[1]),
    ...m.bus.update.mock.calls.map((c) => c[2]),
  ].filter((t) => t != null);
}

const H = 60 * 60 * 1000;

beforeEach(() => {
  vi.resetModules();
  m.ok.clear();
  m.sent = [];
  m.stored = [];
  for (const f of Object.values(m.bus)) f.mockClear();
  m.ok.set("load_kv", () => null);
  m.ok.set("save_kv", () => undefined);
});
afterEach(() => vi.useRealTimers());

describe.each(HOSTILE)("hostile error: %s", (_label, hostile) => {
  beforeEach(() => { m.hostile = hostile; });

  it("jobFailureReason", () => assertSafe(jobFailureReason(hostile), "jobFailureReason"));

  it("Reasoning, Consolidation, Pruning, Midday, Evening, Daily Summary (real continuity → scheduler outcome)", async () => {
    const c = await import("./continuity");
    const s = await import("./scheduler");
    const details = [
      ["Reasoning", s.reasoningOutcome(await c.runReasoningLoop())],
      ["Consolidation", s.consolidationOutcome(await c.consolidateRecentSessions())],
      ["Pruning", s.pruneOutcome(await c.pruneMemory())],
      ["Midday", s.sentOutcome(await c.generateMiddayCorrection())],
      ["Evening", s.sentOutcome(await c.generateEveningIntegration())],
      ["Daily Summary", s.sentOutcome(await c.generateDailySummary())],
    ] as const;
    for (const [name, o] of details) {
      expect(o.ok, name).toBe(false);
      assertSafe(o.detail, name);
    }
  });

  it("any job that throws (e.g. Health Check runDiagnostics) → runJob's catch", async () => {
    const s = await import("./scheduler");
    const o = s.thrownOutcome(hostile, "Health Check");
    expect(o).toEqual({ ok: false, detail: "Health Check failed: generation failed" });
    assertSafe(s.thrownOutcome(new s.TaskTimeoutError("Reasoning Loop timed out after 300000ms"), "Reasoning Loop").detail, "timeout");
  });

  it("custom job: run failure and delivery failure (Activity + lastResult)", async () => {
    const cj = await import("./customJobs");
    const job = { id: "j1", name: "Brief", prompt: "p", schedule: "every 1h", parsedSchedule: null, enabled: true, silent: false, lastRun: 0, lastResult: null, createdAt: 0 };
    const failed = await cj.runJob(job as never);
    assertSafe(failed.output, "lastResult on failure");
    m.ok.set("get_config", () => ({}));
    m.ok.set("chat_with_router", () => ({ content: "hello", provider: "local", model: "gemma4" }));
    const undelivered = await cj.runJob(job as never);
    expect(undelivered.delivered).toBe(false);
    expect(m.bus.fail.mock.calls.length).toBe(2);
    for (const t of activityTexts()) assertSafe(t, "custom job activity");
  });

  it("ritual delivery", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 29, 8, 2));
    await (await import("./rituals")).tickRituals();
    expect(m.bus.fail).toHaveBeenCalledTimes(1);
    for (const t of activityTexts()) assertSafe(t, "ritual");
  });

  it("subagent run and subagent job", async () => {
    const { parseSchedule } = await import("./cronParser");
    const every = parseSchedule("every 1h");
    m.ok.set("load_subagents", () => JSON.stringify([{
      id: "a1", name: "Scout", purpose: "watch", schedule: "every 1h", parsedSchedule: every, status: "active",
      lastRun: 0, lastOutput: null, memory: [], createdAt: 0,
      jobs: [{ id: "k1", name: "Check", prompt: "p", schedule: "every 1h", parsedSchedule: every, enabled: true, lastRun: 0 }],
    }]));
    m.ok.set("save_subagents", () => undefined);
    await (await import("./subagents")).tickSubagents();
    expect(m.bus.fail).toHaveBeenCalledTimes(2);
    for (const t of activityTexts()) assertSafe(t, "subagent");
  });

  it("Skills Evolution", async () => {
    const evo = await (await import("./skillsEvolution")).evolveSkills();
    expect(evo).toEqual({ proposed: 0, created: 0, updated: 0 });
    for (const t of activityTexts()) assertSafe(t, "skills evolution");
  });

  it("Health Check autoHeal: thrown, refused, and per-service failures (Activity, report, Telegram)", async () => {
    const sd = await import("./selfDiagnosis");
    m.ok.set("store_session_log", () => undefined);
    m.ok.set("send_telegram_message", () => undefined);
    m.ok.set("load_terrance_context", () => "{}");
    m.ok.set("save_terrance_context", () => undefined);
    const report = { issues: [{ service: "n8n", status: "down", message: "x", canAutoHeal: true }] } as never;
    const NOW = 1_800_000_000_000;

    sd.__resetAutoHealForTests();
    await sd.autoHeal(report, NOW); // restart_services throws the hostile error
    sd.__resetAutoHealForTests();
    m.ok.set("restart_services", () => { throw typeof hostile === "string" ? `refused: ${hostile}` : new Error(`refused: ${String((hostile as Error)?.message ?? hostile)}`); });
    await sd.autoHeal(report, NOW + 5 * H);
    sd.__resetAutoHealForTests();
    m.ok.set("restart_services", () => [{ service: "docker:stack-n8n-1", ok: false, error: String((hostile as Error)?.message ?? hostile) }]);
    await sd.autoHeal(report, NOW + 10 * H);

    expect(m.bus.fail).toHaveBeenCalledTimes(3);
    for (const t of activityTexts()) assertSafe(t, "autoHeal activity");
    for (const e of m.stored) assertSafe((e as { summary: string }).summary, "auto-heal report");
    for (const t of m.sent) assertSafe(t, "auto-heal Telegram");
  });
});

describe("kill-switch refusals stay neutral Suppressed", () => {
  const refusal = new Error("Outbound sends are disabled for this session (NEXUS_OUTBOUND_DISABLED).");

  it("ritual", async () => {
    m.hostile = refusal;
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 29, 8, 2));
    await (await import("./rituals")).tickRituals();
    expect(m.bus.suppress).toHaveBeenCalledWith("Ritual: Presence Drill", "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)");
    expect(m.bus.fail).not.toHaveBeenCalled();
  });

  it("daily touches (Daily Summary, Midday, Evening)", async () => {
    const s = await import("./scheduler");
    expect(s.sentOutcome({ sent: false, reason: jobFailureReason(refusal) }))
      .toEqual({ ok: false, suppressed: true, detail: "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)" });
  });
});
