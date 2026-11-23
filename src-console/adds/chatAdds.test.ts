import { describe, expect, it, vi } from "vitest";
import { UNWIRED } from "../lib/sampleAdapters";
import { makeLiveAdd } from "./adapters";
import { createChatAdds, detectAddIntent } from "./chatAdds";
import type { AddAdapter, AddPreview, ParseResult } from "./flow";

const preview = (title: string): AddPreview => ({ title, fields: [["Venture", "Taiyo Skin"]], payload: { kind: "task", title, venture: "taiyo-skin", due: null } });

/** A recording adapter standing in for the one live adapter. */
function fake(result: ParseResult = { kind: "preview", preview: preview("Order jars") }) {
  const calls: string[] = [];
  const saves: { p: AddPreview; via?: string }[] = [];
  const a: AddAdapter = {
    command: "x", wired: true,
    parse: async (t) => { calls.push(`parse:${t}`); return { state: "live", value: result }; },
    save: async (p, via) => { saves.push({ p, via }); return { state: "live", value: { id: "1", title: p.title } }; },
    undo: async () => UNWIRED, history: () => UNWIRED,
  };
  return { a, calls, saves };
}

describe("detectAddIntent: explicit phrasings only", () => {
  it.each([
    ["add a goal meditate 10 minutes", "goal", "meditate 10 minutes"],
    ["Amara, add a new habit: cold shower", "goal", "cold shower"],
    ["remind me on Sept 14 2027 to renew the visa", "date", "Sept 14 2027 to renew the visa"],
    ["add task order 50 jars for Taiyo Skin by Friday", "task", "order 50 jars for Taiyo Skin by Friday"],
    ["content idea: how label approval works", "idea", "how label approval works"],
  ])("%s", (text, kind, body) => { expect(detectAddIntent(text)).toEqual({ kind, text: body }); });

  it.each([
    "I should add a goal someday", "what is a good goal?", "remind me", "add task", "add a goal", "the content idea is fine",
    "could you maybe remind me on Friday", "task list please", "",
  ])("ambiguous or empty does nothing: %j", (text) => { expect(detectAddIntent(text)).toBeNull(); });
});

describe("chat and voice share one add path", () => {
  it("an ambiguous turn never reaches the adapter", async () => {
    const f = fake(); const c = createChatAdds({ adapter: () => f.a });
    expect(await c.handle("I should add a goal someday", "chat")).toBeNull();
    expect(f.calls).toEqual([]); expect(f.saves).toEqual([]);
  });

  it("a preview alone never saves; confirm is required, and it is a separate turn", async () => {
    const f = fake(); const c = createChatAdds({ adapter: () => f.a });
    const reply = await c.handle("add task order jars for taiyo", "chat");
    expect(reply).toContain("Check before saving"); expect(reply).toContain('Say "confirm"');
    expect(f.saves).toEqual([]);
    expect(await c.handle("sure, do it", "chat")).toBeNull();
    expect(f.saves).toEqual([]);
    expect(await c.handle("confirm", "chat")).toContain("Saved: Order jars");
    expect(f.saves).toHaveLength(1);
    expect(await c.handle("confirm", "chat")).toBeNull(); // nothing pending any more
    expect(f.saves).toHaveLength(1);
  });

  it("confirm with nothing pending, cancel, and expiry all save nothing", async () => {
    const f = fake(); let t = 0; const c = createChatAdds({ adapter: () => f.a, now: () => t });
    expect(await c.handle("confirm", "voice")).toBeNull();
    await c.handle("add task a thing", "voice"); expect(await c.handle("cancel", "voice")).toContain("Nothing was saved");
    await c.handle("add task a thing", "voice"); t = 3 * 60_000; expect(await c.handle("confirm", "voice")).toBeNull();
    expect(f.saves).toEqual([]);
  });

  it("an ask (unknown year) or elsewhere result sets no pending save", async () => {
    const f = fake({ kind: "ask", question: "Which year is it?", options: [], note: "No guessing." });
    const c = createChatAdds({ adapter: () => f.a });
    expect(await c.handle("remind me on Sept 14 to renew", "chat")).toContain("Which year is it?");
    expect(await c.handle("confirm", "chat")).toBeNull(); expect(f.saves).toEqual([]);
  });

  it("saves the backend's own preview payload unchanged, with via chat or voice and the venture tag intact", async () => {
    for (const via of ["chat", "voice"] as const) {
      const f = fake(); const c = createChatAdds({ adapter: () => f.a });
      await c.handle("add task order jars for taiyo", via); await c.handle("save it", via);
      expect(f.saves[0].via).toBe(via);
      expect(f.saves[0].p).toEqual(preview("Order jars"));
      expect((f.saves[0].p.payload as { venture: string }).venture).toBe("taiyo-skin");
    }
  });

  it("through the real live adapter, chat and voice use add_preview then add_save, and nothing else writes", async () => {
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "add_history") return [];
      if (command === "add_preview") return { kind: "preview", preview: preview(String(args?.text)) };
      if (command === "add_save") return { id: "n1", title: "t" };
      throw new Error(`unexpected ${command}`);
    });
    const live = makeLiveAdd("task", invoke as never);
    const c = createChatAdds({ adapter: () => live });
    await c.handle("add task order jars", "chat"); await c.handle("confirm", "chat");
    await c.handle("add task ship labels", "voice"); await c.handle("confirm", "voice");
    const saves = invoke.mock.calls.filter(([n]) => n === "add_save");
    expect(saves.map(([, a]) => (a as { via: string }).via)).toEqual(["chat", "voice"]);
    expect(invoke.mock.calls.filter(([n]) => n === "add_preview")).toHaveLength(2);
    expect(new Set(invoke.mock.calls.map(([n]) => n))).toEqual(new Set(["add_preview", "add_save", "add_history"]));
  });
});


describe("review fixes", () => {
  it("M1: the optional Amara lead works on confirm and cancel", async () => {
    for (const word of ["Amara, save it", "hey amara confirm", "Amara, cancel"]) {
      const f = fake(); const c = createChatAdds({ adapter: () => f.a });
      await c.handle("add task order jars", "voice");
      await c.handle(word, "voice");
      expect(f.saves).toHaveLength(/cancel/i.test(word) ? 0 : 1);
    }
  });

  it("L2: a slow older parse can't re-arm a pending save after a newer turn", async () => {
    let releaseFirst: (v: unknown) => void = () => undefined;
    const slow = fake(); let n = 0;
    slow.a.parse = () => (++n === 1 ? new Promise((r) => { releaseFirst = r as never; }) : Promise.resolve({ state: "live", value: { kind: "ask", question: "Which day?", options: [] } } as never));
    const c = createChatAdds({ adapter: () => slow.a });
    const first = c.handle("add task slow one", "chat");
    expect(await c.handle("add task slow two", "chat")).toContain("Which day?"); // newer turn, no pending
    releaseFirst({ state: "live", value: { kind: "preview", preview: preview("late") } });
    expect(await first).toContain("newer request");
    expect(c.hasPending()).toBe(false);
    expect(await c.handle("confirm", "chat")).toBeNull();
    expect(slow.saves).toEqual([]);
  });
});
