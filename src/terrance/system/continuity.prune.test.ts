import { describe, it, expect, vi, beforeEach } from "vitest";

const m = vi.hoisted(() => ({
  calls: [] as { cmd: string; args?: unknown }[],
  archiveFails: false,
  saved: 0,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: unknown) => {
    m.calls.push({ cmd, args });
    if (cmd === "list_session_logs") return [];
    if (cmd === "archive_old_session_logs") return 0;
    if (cmd === "prune_context_memory") return 0;
    if (cmd === "archive_activity_log") {
      if (m.archiveFails) throw "activity archive 2026-04 failed verification; sources kept";
      return { archived: 12, scanned: 20, selected: 12, kept: 8, skipped: 0, dryRun: false, months: [] };
    }
    if (cmd === "load_terrance_context") return { activeGoals: [], relevantMemory: [] };
    if (cmd === "read_goals") return [];
    if (cmd === "save_terrance_context") { m.saved++; return; }
    throw new Error(`unexpected ${cmd}`);
  }),
}));
vi.mock("@/lib/api", () => ({ getConfig: vi.fn(async () => ({})) }));
vi.mock("@/lib/llm-router", () => ({ buildRouterConfig: vi.fn() }));
vi.mock("./deliberation", () => ({ deliberate: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: { register: vi.fn(), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() } }));

import { pruneMemory, ACTIVITY_KEEP_DAYS, ACTIVITY_MAX_FILES } from "./continuity";

describe("daily prune archives the activity log", () => {
  beforeEach(() => { m.calls = []; m.archiveFails = false; m.saved = 0; vi.spyOn(console, "error").mockImplementation(() => {}); });

  it("asks Rust to archive files older than 30 days with a 5000-file cap (not a dry run)", async () => {
    const r = await pruneMemory();
    const call = m.calls.find((c) => c.cmd === "archive_activity_log");
    expect(call?.args).toEqual({ keepDays: 30, maxFiles: 5000, dryRun: false });
    expect([ACTIVITY_KEEP_DAYS, ACTIVITY_MAX_FILES]).toEqual([30, 5000]);
    expect(r.activityArchived).toBe(12);
    expect(m.calls.some((c) => c.cmd === "prune_activity_log")).toBe(false);
    expect(m.saved).toBe(1);
  });

  it("an archive failure is non-fatal: prune still completes and records its timestamp", async () => {
    m.archiveFails = true;
    const r = await pruneMemory();
    expect(r.activityArchived).toBe(0);
    expect(m.saved).toBe(1);
  });
});
