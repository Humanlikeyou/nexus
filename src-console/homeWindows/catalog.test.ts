import { describe, expect, it } from "vitest";
import { parseWindowPrompt } from "./catalog";
import { resolveVoiceTarget } from "./intents";
import { requestFromCatalog } from "./catalog";
import { windowsReducer, resetWindowIds } from "./reducer";
import { initialWindowsState } from "./types";

describe("parseWindowPrompt", () => {
  it("opens on pull up / show me / open / bring up, not on a bare name", () => {
    expect(parseWindowPrompt("pull up bitcoin on TradingView", "voice")?.catalogId).toBe("tradingview");
    expect(parseWindowPrompt("show me the lease pdf", "chat")?.catalogId).toBe("lease");
    expect(parseWindowPrompt("open instagram", "chat")?.catalogId).toBe("instagram");
    expect(parseWindowPrompt("bring up that youtube video", "voice")?.catalogId).toBe("youtube");
    expect(parseWindowPrompt("bitcoin", "voice")).toBeNull();
  });

  it("does not treat close / expand as an open", () => {
    expect(parseWindowPrompt("close the chart", "voice")).toBeNull();
    expect(parseWindowPrompt("make the video big", "chat")).toBeNull();
  });
});

describe("resolveVoiceTarget", () => {
  it("resolves close the chart and close everything", () => {
    resetWindowIds();
    let s = initialWindowsState();
    s = windowsReducer(s, { type: "open", request: requestFromCatalog("tradingview", "voice"), at: 1, contentState: "ready" });
    s = windowsReducer(s, { type: "open", request: requestFromCatalog("youtube", "voice"), at: 2, contentState: "ready" });
    expect(resolveVoiceTarget(s, "close the chart")).toEqual({ type: "close", target: s.windows[0].id });
    expect(resolveVoiceTarget(s, "close everything")).toEqual({ type: "close", target: "all" });
    expect(resolveVoiceTarget(s, "make the video big")).toEqual({ type: "focus", id: s.windows[1].id });
  });
});
