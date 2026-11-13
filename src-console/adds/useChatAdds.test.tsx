import { renderHook, act, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceTurn } from "../types";

const parse = vi.fn(async (t: string) => ({ state: "live", value: { kind: "preview", preview: { title: t, fields: [], payload: {} } } }));
const save = vi.fn(async (p: { title: string }, via?: string) => ({ state: "live", value: { id: "1", title: p.title, via } }));
vi.mock("./adapters", () => ({ addAdapter: () => ({ parse, save }) }));

import { useChatAdds } from "./useChatAdds";

const NOW = Date.parse("2026-11-13T09:00:00+09:00");
const ago = (s: number) => new Date(NOW - s * 1000).toISOString();
const turn = (secondsAgo: number, text: string, extra: Partial<VoiceTurn> = {}): VoiceTurn => ({ time: ago(secondsAgo), text, heard: `amara ${text}`, route: "hermes", verdict: "ok", ...extra });

beforeEach(() => { vi.useFakeTimers({ toFake: ["Date"] }); vi.setSystemTime(NOW); parse.mockClear(); save.mockClear(); });

describe("useChatAdds voice", () => {
  it("M2: starts empty, then the old history arrives: nothing happens", async () => {
    const post = vi.fn();
    const { rerender } = renderHook(({ turns }) => useChatAdds(post, turns), { initialProps: { turns: [] as VoiceTurn[] } });
    rerender({ turns: [turn(3000, "add task old one"), turn(500, "add task older"), turn(300, "confirm")] });
    await act(async () => { await Promise.resolve(); });
    expect(parse).not.toHaveBeenCalled(); expect(save).not.toHaveBeenCalled(); expect(post).not.toHaveBeenCalled();
  });

  it("previews a fresh spoken add without saving; a spoken 'Amara, save it' then saves via voice (M1 lead on confirm)", async () => {
    const post = vi.fn();
    const { rerender } = renderHook(({ turns }) => useChatAdds(post, turns), { initialProps: { turns: [] as VoiceTurn[] } });
    const a = turn(5, "add task order jars");
    rerender({ turns: [a] });
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(parse).toHaveBeenCalledWith("order jars"); expect(save).not.toHaveBeenCalled();
    expect(post.mock.calls[0][0][0].local).toBe(true);
    rerender({ turns: [a, turn(1, "Amara, save it")] });
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][1]).toBe("voice");
  });

  it("M1: ignored, noise and low-confidence turns are never commands; text wins over heard, heard only when text is absent", async () => {
    const post = vi.fn();
    const { rerender } = renderHook(({ turns }) => useChatAdds(post, turns), { initialProps: { turns: [] as VoiceTurn[] } });
    rerender({ turns: [
      turn(9, "add task a", { route: "ignored" }), turn(8, "add task b", { ignored: "no wake word" }),
      turn(7, "add task c", { verdict: "noise" }), turn(6, "add task d", { verdict: "low" }),
      // heard has the wake word ("amara ...") but the request in text is not an add: must not match
      { time: ago(5), heard: "add task from heard", text: "what time is it", route: "hermes" },
    ] });
    await act(async () => { await Promise.resolve(); });
    expect(parse).not.toHaveBeenCalled();
    rerender({ turns: [{ time: ago(3), heard: "add task legacy line", route: "hermes" }] });
    await waitFor(() => expect(parse).toHaveBeenCalledWith("legacy line"));
  });

  it("L2: every new turn in one poll is handled, oldest first", async () => {
    const post = vi.fn();
    const { rerender } = renderHook(({ turns }) => useChatAdds(post, turns), { initialProps: { turns: [] as VoiceTurn[] } });
    rerender({ turns: [turn(2, "add task second"), turn(10, "add task first")] });
    await waitFor(() => expect(parse).toHaveBeenCalledTimes(2));
    expect(parse.mock.calls.map((c) => c[0])).toEqual(["first", "second"]);
  });

  it("typed chat: an add turn is consumed (not sent to Hermes) and posted as local, a normal turn is not", async () => {
    const post = vi.fn();
    const { result } = renderHook(() => useChatAdds(post, []));
    let a = true, b = false;
    await act(async () => { a = await result.current.handleTyped("what's the weather"); b = await result.current.handleTyped("add task ship labels"); });
    expect([a, b]).toEqual([false, true]);
    expect(post).toHaveBeenCalledTimes(1);
    expect(post.mock.calls[0][0].every((m: { local?: boolean }) => m.local === true)).toBe(true);
  });
});
