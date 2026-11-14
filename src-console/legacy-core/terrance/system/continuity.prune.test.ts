import { describe, it, expect, vi, beforeEach } from "vitest";

const m = vi.hoisted(() => ({
  calls: [] as { cmd: string; args?: unknown }[],
  archiveFails: false,
  archivePartial: false,
  archivePartialRaw: false,
  sessionArchiveFails: false,
  saved: 0,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: unknown) => {
    m.calls.push({ cmd, args });
    if (cmd === "list_session_logs") return [];
    if (cmd === "archive_old_session_logs") { if (m.sessionArchiveFails) throw "disk full"; return 0; }
    if (cmd === "prune_context_memory") return 0;
    if (cmd === "archive_activity_log") {
      if (m.archiveFails) throw "activity archive 2026-04 failed verification; sources kept";
      if (m.archivePartialRaw) throw "Archived 1 files from 2 months; stopped at 2026-10: Permission denied (os error 13) at /Users/taisun/Library/Application Support/x/activity/archive/.2026-10.jsonl.gz.tmp; remaining files kept.";
      if (m.archivePartial) throw "Archived 43 files from 1 months; stopped at 2026-04: archive failed verification; remaining files kept.";
      return { archived: 12, scanned: 20, selected: 12, kept: 8, skipped: 0, dryRun: false, months: [] };
    }
    if (cmd === "load_terrance_context") return { activeGoals: [], relevantMemory: [] };
    if (cmd === "read_goals") return [];
    if (cmd === "save_terrance_context") { m.saved++; return; }
    throw new Error(`unexpected ${cmd}`);
  }),
}));
vi.mock("../../lib/api", () => ({ getConfig: vi.fn(async () => ({})) }));
vi.mock("../../lib/llm-router", () => ({ buildRouterConfig: vi.fn() }));
vi.mock("./deliberation", () => ({ deliberate: vi.fn() }));
vi.mock("./activityBus", () => ({ activityBus: { register: vi.fn(), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() } }));

import { pruneMemory, partialArchiveMessage, ACTIVITY_KEEP_DAYS, ACTIVITY_MAX_FILES } from "./continuity";

describe("weekly prune archives the activity log", () => {
  beforeEach(() => { m.calls = []; m.archiveFails = false; m.archivePartial = false; m.archivePartialRaw = false; m.sessionArchiveFails = false; m.saved = 0; vi.spyOn(console, "error").mockImplementation(() => {}); });

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
    expect(r.activityError).toBe("archive step failed");
    expect(r.error).toBeUndefined();
    expect(m.saved).toBe(1);
  });

  it("a partial archive failure reports what was archived (singular month) and where it stopped", async () => {
    m.archivePartial = true;
    const r = await pruneMemory();
    expect(r.activityError).toBe("Archived 43 files from 1 month; stopped at 2026-04: archive failed verification; remaining files kept.");
    expect(r.error).toBeUndefined();
  });

  it("a partial archive failure reports the real archived count, not 0", async () => {
    m.archivePartial = true;
    const r = await pruneMemory();
    expect(r.activityArchived).toBe(43);
  });

  it("a partial archive failure never shows raw I/O text or paths", async () => {
    m.archivePartialRaw = true;
    const r = await pruneMemory();
    expect(r.activityError).toBe("Archived 1 file from 2 months; stopped at 2026-10: archive write failed; remaining files kept.");
    expect(r.activityError).not.toMatch(/Users|\/|os error|Permission/);
    expect(r.activityArchived).toBe(1);
  });

  it("pluralises files and months", () => {
    const base = { stoppedAt: "2026-10", reason: "archive write failed" };
    expect(partialArchiveMessage({ ...base, files: 1, months: 1 })).toMatch(/^Archived 1 file from 1 month;/);
    expect(partialArchiveMessage({ ...base, files: 2, months: 3 })).toMatch(/^Archived 2 files from 3 months;/);
    expect(partialArchiveMessage({ ...base, files: 0, months: 0 })).toMatch(/^Archived 0 files from 0 months;/);
  });

  it("a failed run reports error + reason and leaves lastPruneTimestamp unchanged", async () => {
    m.sessionArchiveFails = true;
    const r = await pruneMemory();
    expect(r).toMatchObject({ error: true, reason: "storage step failed", archived: 0 });
    expect(m.saved).toBe(0);
  });
});
