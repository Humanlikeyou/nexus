import { describe, expect, it, vi } from "vitest";
import fixture from "../../test/fixtures/turn-intents.json";
import { parseNavyPrompt } from "../navyMode/parse";
import { parseWindowPrompt } from "../homeWindows/catalog";
import { looksLikeHomeCommand } from "../lib/homeAssistant";

vi.mock("../bridge", () => ({ amaraTurn: vi.fn(), amaraTurnCancel: vi.fn() }));
import { createSendHome, runTurn } from "./turnChannel";
import { amaraTurn, amaraTurnCancel } from "../bridge";

describe("createSendHome", () => {
  const mk = (enabled: boolean, windowIntent: { type: string } | null = null) => {
    const d = {
      handleChatAdd: vi.fn(async () => false), handleNavyPrompt: vi.fn(() => false), handleWindowPrompt: vi.fn(() => false),
      resolveWindowIntent: vi.fn(() => windowIntent), applyWindowIntent: vi.fn(), enterNavy: vi.fn(), exitNavy: vi.fn(),
      sendToTerrence: vi.fn(async () => undefined), enabled: () => enabled,
    };
    return { d, send: createSendHome(d) };
  };

  it("flag off: adds, navy, windows then the old send, in that order", async () => {
    const { d, send } = mk(false);
    await send("hello", { deep: true });
    expect(d.handleChatAdd).toHaveBeenCalledWith("hello");
    expect(d.handleNavyPrompt).toHaveBeenCalledWith("hello", "chat");
    expect(d.handleWindowPrompt).toHaveBeenCalledWith("hello", "chat");
    expect(d.sendToTerrence).toHaveBeenCalledWith("hello", { deep: true, turnPipeline: false });
    expect(d.resolveWindowIntent).not.toHaveBeenCalled();
  });

  it("flag off: a handled add stops the send", async () => {
    const { d, send } = mk(false);
    d.handleChatAdd.mockResolvedValue(true);
    await send("add a goal");
    expect(d.sendToTerrence).not.toHaveBeenCalled();
  });

  it("flag on: a handled add never calls amara_turn; an unhandled one does", async () => {
    const { d, send } = mk(true);
    d.handleChatAdd.mockResolvedValueOnce(true);
    await send("add a goal: run");
    expect(d.sendToTerrence).not.toHaveBeenCalled();
    await send("hello");
    expect(d.handleChatAdd).toHaveBeenCalledTimes(2);
    expect(d.sendToTerrence).toHaveBeenCalledTimes(1);
  });

  it("flag on: no renderer pre-route for navy or window open before the call", async () => {
    const { d, send } = mk(true);
    await send("go dark");
    expect(d.handleNavyPrompt).not.toHaveBeenCalled();
    expect(d.handleWindowPrompt).not.toHaveBeenCalled();
    expect(d.sendToTerrence).toHaveBeenCalledTimes(1);
    expect((d.sendToTerrence.mock.calls[0] as unknown as [string, { turnPipeline: boolean }])[1].turnPipeline).toBe(true);
  });

  it("flag on: stateful window commands (close, focus) stay in the renderer and are not sent", async () => {
    const { d, send } = mk(true, { type: "close" });
    await send("close it");
    expect(d.applyWindowIntent).toHaveBeenCalledWith({ type: "close" });
    expect(d.sendToTerrence).not.toHaveBeenCalled();
  });

  it("flag on: Rust navy and window_open actions drive the renderer effects once", async () => {
    const { d, send } = mk(true);
    await send("x");
    const onAction = (d.sendToTerrence.mock.calls[0] as unknown as [string, { onAction: (k: string, p: unknown) => void }])[1].onAction;
    onAction("navy", { on: true });
    onAction("navy", { on: false });
    expect(d.enterNavy).toHaveBeenCalledTimes(1);
    expect(d.exitNavy).toHaveBeenCalledTimes(1);
    expect(d.handleWindowPrompt).not.toHaveBeenCalled();
    onAction("window_open", { catalog_id: "instagram" });
    expect(d.handleWindowPrompt).toHaveBeenCalledWith("x", "chat");
  });
});

describe("runTurn", () => {
  it("shows only fixed copy for an error code, never the event message", async () => {
    let on!: (e: unknown) => void;
    vi.mocked(amaraTurn).mockImplementation(((_i: unknown, _s: unknown, cb: (e: unknown) => void) => { on = cb; return new Promise<string>(() => undefined); }) as never);
    const messages: string[] = [];
    const t = runTurn({ conversationId: "c", text: "t" }, "typed", { onDone: vi.fn(), onError: (_c, m) => messages.push(m) });
    on({ type: "error", code: "something_new", message: "<b>sk-secret</b> /Users/x/path model gemma" });
    await t.finished;
    expect(messages).toEqual(["Amara couldn't answer right now."]);
    const known: string[] = [];
    vi.mocked(amaraTurn).mockImplementation(((_i: unknown, _s: unknown, cb: (e: unknown) => void) => { on = cb; return new Promise<string>(() => undefined); }) as never);
    const t2 = runTurn({ conversationId: "c", text: "t" }, "typed", { onDone: vi.fn(), onError: (_c, m) => known.push(m) });
    on({ type: "error", code: "local_only", message: "hostile" });
    await t2.finished;
    expect(known[0]).toMatch(/^Local only right now\./);
  });

  it("ends once, ignores later events, and cancel before the id arrives still cancels", async () => {
    let on!: (e: unknown) => void;
    let resolveId!: (id: string) => void;
    vi.mocked(amaraTurn).mockImplementation(((_i: unknown, _s: unknown, cb: (e: unknown) => void) => { on = cb; return new Promise<string>((r) => { resolveId = r; }); }) as never);
    vi.mocked(amaraTurnCancel).mockResolvedValue(true);
    const onDone = vi.fn(); const onError = vi.fn();
    const t = runTurn({ conversationId: "c", text: "t" }, "typed", { onDone, onError });
    t.cancel();
    await t.finished;
    on({ type: "done", interaction_id: "i", text: "x", path: "router", duration_ms: 1, sections_used: [] });
    expect(onDone).not.toHaveBeenCalled();
    resolveId("id-1");
    await Promise.resolve();
    expect(amaraTurnCancel).toHaveBeenCalledWith("id-1");
    expect(onError).not.toHaveBeenCalled();
  });
});

describe("turn-intents fixture parity (shared with amara_turn.rs)", () => {
  const cases = fixture as { text: string; kind: string; catalogId?: string }[];
  it("has cases", () => expect(cases.length).toBeGreaterThanOrEqual(10));
  it.each(cases.map((c) => [c.text, c] as const))("%s", (_t, c) => {
    const navy = parseNavyPrompt(c.text);
    const win = parseWindowPrompt(c.text, "chat");
    const got = navy === "on" ? "navy_on" : navy === "off" ? "navy_off" : win ? "window" : looksLikeHomeCommand(c.text, []) ? "home" : "none";
    expect(got).toBe(c.kind);
    if (c.catalogId) expect(win?.catalogId).toBe(c.catalogId);
  });
});
