import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VoiceTurn } from "../types";
import { homeWindowsStore } from "./store";
import { useVoiceWindowPrompts } from "./useHomeWindows";

afterEach(() => { homeWindowsStore.reset(); });

describe("useVoiceWindowPrompts replays no history", () => {
  it("an empty first poll followed by old history opens nothing; a fresh turn does", async () => {
    const changed = vi.fn();
    const off = homeWindowsStore.subscribe(changed);
    const old = new Date(Date.now() - 600_000).toISOString();
    const hist: VoiceTurn = { time: old, text: "open instagram", route: "hermes" };
    const { rerender } = renderHook(({ t }) => useVoiceWindowPrompts(t), { initialProps: { t: [] as VoiceTurn[] } });
    rerender({ t: [hist] });
    await new Promise((r) => setTimeout(r, 50));
    expect(changed).not.toHaveBeenCalled();
    // positive control, so the listener is known to be live
    rerender({ t: [hist, { time: new Date().toISOString(), text: "open instagram", route: "hermes" }] });
    await waitFor(() => expect(changed).toHaveBeenCalled());
    off();
  });
});
