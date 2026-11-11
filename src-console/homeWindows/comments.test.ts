import { describe, expect, it } from "vitest";
import { requestFromCatalog } from "./catalog";
import { canComment, shouldSpeakComment } from "./intents";
import { windowsReducer, resetWindowIds } from "./reducer";
import { initialWindowsState, type ContentState, type HomeWindow } from "./types";

function open(state = initialWindowsState(), id = "tradingview", by: "voice" | "chat" = "voice", contentState: ContentState = "ready") {
  resetWindowIds();
  return windowsReducer(state, { type: "open", request: requestFromCatalog(id, by), at: 1, contentState });
}

const win = (s: ReturnType<typeof open>, patch: Partial<HomeWindow> = {}): HomeWindow => ({ ...s.windows[0], ...patch });

describe("comment timing (design-1111w)", () => {
  it("never comments while signed out, signing in, blocked, offline or unwired", () => {
    for (const state of ["signed_out", "signing_in", "blocked", "offline", "unwired"] as const) {
      const w = win(open(initialWindowsState(), "instagram", "voice", state));
      expect(canComment(w), state).toBe(false);
      expect(shouldSpeakComment({ ...w, comment: { phase: "ready", text: "hi" } })).toBe(false);
    }
  });

  it("speaks only when he asked by voice; chat is caption only", () => {
    const spoken = win(open(initialWindowsState(), "tradingview", "voice", "ready"), { comment: { phase: "ready", text: "up 3%" } });
    const typed = win(open(initialWindowsState(), "tradingview", "chat", "ready"), { comment: { phase: "ready", text: "up 3%" } });
    expect(canComment(spoken)).toBe(true);
    expect(shouldSpeakComment(spoken)).toBe(true);
    expect(canComment(typed)).toBe(true);
    expect(shouldSpeakComment(typed)).toBe(false);
  });
});
