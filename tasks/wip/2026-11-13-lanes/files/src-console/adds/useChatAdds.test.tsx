import { renderHook, act, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const parse = vi.fn(async (t: string) => ({ state: "live", value: { kind: "preview", preview: { title: t, fields: [], payload: {} } } }));
const save = vi.fn(async (p: { title: string }, via?: string) => ({ state: "live", value: { id: "1", title: p.title, via } }));
vi.mock("./adapters", () => ({ addAdapter: () => ({ parse, save }) }));

import { useChatAdds } from "./useChatAdds";

describe("useChatAdds", () => {
  it("skips the historical voice poll, then previews a new spoken add without saving, and confirm saves via voice", async () => {
    const post = vi.fn();
    const old = [{ time: "2026-11-12T09:00:00+09:00", heard: "add task old one" }];
    const { rerender } = renderHook(({ turns }) => useChatAdds(post, turns), { initialProps: { turns: old } });
    expect(parse).not.toHaveBeenCalled();
    rerender({ turns: [...old, { time: "2026-11-12T09:01:00+09:00", heard: "add task order jars" }] });
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(parse).toHaveBeenCalledWith("order jars"); expect(save).not.toHaveBeenCalled();
    rerender({ turns: [...old, { time: "2026-11-12T09:01:00+09:00", heard: "add task order jars" }, { time: "2026-11-12T09:02:00+09:00", heard: "confirm" }] });
    await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    expect(save.mock.calls[0][1]).toBe("voice");
  });

  it("typed chat: an add turn is consumed (not sent to Hermes), a normal turn is not", async () => {
    const post = vi.fn();
    const { result } = renderHook(() => useChatAdds(post, []));
    let a = true, b = false;
    await act(async () => { a = await result.current.handleTyped("what's the weather"); b = await result.current.handleTyped("add task ship labels"); });
    expect([a, b]).toEqual([false, true]);
    expect(post).toHaveBeenCalledTimes(1);
  });
});
