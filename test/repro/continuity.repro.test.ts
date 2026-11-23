/**
 * Repro: continuity load error → context wipe (TEST_PLAN bug #1).
 * Expected to FAIL until loadContext distinguishes "missing" from "unreadable".
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("../../src-console/legacy-core/lib/api", () => ({ getConfig: vi.fn(async () => ({})) }));
vi.mock("../../src-console/legacy-core/lib/llm-router", () => ({ buildRouterConfig: vi.fn(() => ({})) }));
vi.mock("../../src-console/legacy-core/terrance/system/deliberation", () => ({ deliberate: vi.fn() }));

import { summarizeAndStoreSession } from "../../src-console/legacy-core/terrance/system/continuity";

describe("continuity: load failure must not clobber persisted context", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case "load_terrance_context":
          throw new Error("expected value at line 1 column 1"); // corrupt JSON
        case "read_goals":
          return [];
        case "chat_with_router":
          return { content: "- summary", provider: "p", model: "m" };
        default:
          return undefined;
      }
    });
  });

  it("does not save an emptied context after a failed load", async () => {
    await summarizeAndStoreSession([
      { role: "user", text: "hello" },
      { role: "assistant", text: "hi" },
    ] as any);
    const saves = invoke.mock.calls.filter(([c]) => c === "save_terrance_context");
    // Intended: refuse to write when the on-disk context could not be read.
    expect(saves).toHaveLength(0);
  });
});
