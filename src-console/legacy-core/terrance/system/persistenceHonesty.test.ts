// 0052: a failed write is reported as a failure, never as success (skills evolution, subagents).
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: { cmd: string; args: unknown }[] = [];
let failSave = false;
let skillReply: unknown = { name: "x", version: 1, state: "READ_BACK_VERIFIED", executionId: "e", note: null };

vi.mock("../../../bridge", () => ({
  invokeBound: vi.fn(async (cmd: string, args?: unknown) => {
    calls.push({ cmd, args });
    if (cmd === "load_subagents") return "[]";
    if (cmd === "save_subagents" && failSave) throw new Error("disk full");
    if (cmd === "propose_skill_version") {
      if (skillReply instanceof Error) throw skillReply;
      return skillReply;
    }
    if (cmd === "read_skills") return [];
    if (cmd === "list_session_logs") {
      return Array.from({ length: 5 }, (_, i) => ({ id: `s${i}`, timestamp: Date.now(), summary: "goal tracking review", keyTopics: ["goals"] }));
    }
    if (cmd === "chat_with_router") {
      const text = (args as { text: string }).text;
      if (text.includes("Output ONLY the JSON array")) {
        return { content: '[{"name":"goal-tracking","description":"d","reason":"r","isUpdate":false}]' };
      }
      return { content: "# goal-tracking\n> d" };
    }
    if (cmd === "save_skill") throw new Error("save_skill must not be used any more");
    return null;
  }),
}));
vi.mock("../../lib/api", () => ({ getConfig: vi.fn(async () => ({})) }));
vi.mock("../../lib/llm-router", () => ({ buildRouterConfig: vi.fn(() => ({})) }));

describe("subagents report failed saves", () => {
  beforeEach(async () => {
    calls.length = 0;
    failSave = false;
    const m = await import("./subagents");
    m.invalidateSubagentsCache();
  });

  it("create returns null when the save fails and nothing is left in memory", async () => {
    const m = await import("./subagents");
    failSave = true;
    expect(await m.createSubagent("Scout", "watch", "daily at 9am")).toBeNull();
    failSave = false;
    expect(await m.listSubagents()).toEqual([]);
  });

  it("create returns the agent when the save succeeds", async () => {
    const m = await import("./subagents");
    const a = await m.createSubagent("Scout", "watch", "daily at 9am");
    expect(a?.name).toBe("Scout");
    expect(calls.some((c) => c.cmd === "save_subagents")).toBe(true);
  });
});

describe("skills evolution counts only verified stores", () => {
  beforeEach(() => {
    calls.length = 0;
    vi.resetModules();
  });

  it("a read-back-verified version counts, through propose_skill_version, never save_skill", async () => {
    skillReply = { name: "goal-tracking", version: 1, state: "READ_BACK_VERIFIED", executionId: "e1", note: null };
    const { evolveSkills } = await import("./skillsEvolution");
    const r = await evolveSkills();
    expect(r).toMatchObject({ created: 1, failed: 0 });
    expect(calls.some((c) => c.cmd === "propose_skill_version")).toBe(true);
    expect(calls.some((c) => c.cmd === "save_skill")).toBe(false);
  });

  it("a rejected proposal is not counted as created", async () => {
    skillReply = { name: "goal-tracking", version: 1, state: "REJECTED", executionId: null, note: "no_title" };
    const { evolveSkills } = await import("./skillsEvolution");
    expect(await evolveSkills()).toMatchObject({ created: 0, failed: 0 });
  });

  it("a storage failure is counted as failed, not created", async () => {
    skillReply = new Error("Skill goal-tracking v1 was not stored");
    const { evolveSkills } = await import("./skillsEvolution");
    expect(await evolveSkills()).toMatchObject({ created: 0, failed: 1 });
  });
});
