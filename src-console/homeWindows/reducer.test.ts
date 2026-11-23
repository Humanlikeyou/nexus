import { describe, expect, it, beforeEach } from "vitest";
import { requestFromCatalog } from "./catalog";
import { countTowardCap, layoutSlots, lruId, stageWindows, toastLabel, windowsReducer } from "./reducer";
import { initialWindowsState, MAX_WINDOWS, UNDO_MS } from "./types";
import { resetWindowIds } from "./reducer";
import { resolveVoiceTarget } from "./intents";

beforeEach(() => resetWindowIds());

const open = (id: string, at: number, by: "voice" | "chat" = "voice") =>
  ({ type: "open" as const, request: requestFromCatalog(id, by), at });

describe("windows reducer", () => {
  it("opens 1, then 2, then 3, then a 4th replaces the least recently looked at (not the oldest opened)", () => {
    let s = initialWindowsState();
    s = windowsReducer(s, open("tradingview", 1000));
    s = windowsReducer(s, open("youtube", 2000));
    s = windowsReducer(s, open("lease", 3000));
    expect(s.windows.map((w) => w.catalogId)).toEqual(["tradingview", "youtube", "lease"]);
    expect(s.windows).toHaveLength(MAX_WINDOWS);
    // He looks at the oldest (TradingView) so YouTube becomes LRU.
    const tv = s.windows.find((w) => w.catalogId === "tradingview")!;
    s = windowsReducer(s, { type: "focus", id: tv.id, at: 4000 });
    const yt = s.windows.find((w) => w.catalogId === "youtube")!;
    expect(lruId(s.windows)).toBe(yt.id);
    s = windowsReducer(s, open("instagram", 5000));
    expect(s.windows.map((w) => w.catalogId).sort()).toEqual(["instagram", "lease", "tradingview"]);
    expect(s.windows.some((w) => w.catalogId === "youtube")).toBe(false);
    expect(s.toast?.closedTitle).toMatch(/Label approval/);
    expect(toastLabel(s.toast!)).toMatch(/looked at least recently/);
    expect(toastLabel(s.toast!)).toMatch(/YouTube/);
    expect(s.toast?.until).toBe(5000 + UNDO_MS);
    const instagram = s.windows.find((w) => w.catalogId === "instagram")!;
    expect(s.focusedId).toBe(instagram.id);
  });

  it("Undo brings the evicted window back and closes the one that replaced it", () => {
    let s = initialWindowsState();
    s = windowsReducer(s, open("tradingview", 1));
    s = windowsReducer(s, open("youtube", 2));
    s = windowsReducer(s, open("lease", 3));
    s = windowsReducer(s, open("instagram", 4));
    expect(s.windows.some((w) => w.catalogId === "tradingview")).toBe(false);
    s = windowsReducer(s, { type: "undo", at: 4 + 1000 });
    expect(s.windows.map((w) => w.catalogId).sort()).toEqual(["lease", "tradingview", "youtube"]);
    expect(s.windows.some((w) => w.catalogId === "instagram")).toBe(false);
    expect(s.toast).toBeNull();
  });

  it("Undo after 8 s does nothing", () => {
    let s = initialWindowsState();
    s = windowsReducer(s, open("tradingview", 1));
    s = windowsReducer(s, open("youtube", 2));
    s = windowsReducer(s, open("lease", 3));
    s = windowsReducer(s, open("instagram", 4));
    s = windowsReducer(s, { type: "undo", at: 4 + UNDO_MS + 1 });
    expect(s.windows.some((w) => w.catalogId === "instagram")).toBe(true);
    expect(s.windows.some((w) => w.catalogId === "tradingview")).toBe(false);
  });

  it("popped-out windows count toward the max of 3, so a 4th still replaces one", () => {
    let s = initialWindowsState();
    s = windowsReducer(s, open("tradingview", 1));
    s = windowsReducer(s, open("youtube", 2));
    const tv = s.windows.find((w) => w.catalogId === "tradingview")!;
    s = windowsReducer(s, { type: "popOut", id: tv.id, at: 3 });
    expect(stageWindows(s.windows)).toHaveLength(1);
    expect(countTowardCap(s.windows)).toBe(2);
    s = windowsReducer(s, open("lease", 4));
    expect(countTowardCap(s.windows)).toBe(3);
    s = windowsReducer(s, open("instagram", 5));
    expect(countTowardCap(s.windows)).toBe(3);
    expect(s.toast).not.toBeNull();
  });

  it("closing the last window returns Home after returnFinished, with Undo of the closed window", () => {
    let s = initialWindowsState();
    s = windowsReducer(s, open("tradingview", 1));
    const id = s.windows[0].id;
    s = windowsReducer(s, { type: "close", target: id, at: 2 });
    expect(s.phase).toBe("returning");
    expect(s.windows).toHaveLength(0);
    s = windowsReducer(s, { type: "returnFinished" });
    expect(s.phase).toBe("home");
    s = windowsReducer(s, { type: "undo", at: 3 });
    expect(s.windows).toHaveLength(1);
    expect(s.windows[0].catalogId).toBe("tradingview");
  });

  it("the focused window takes the big slot; ⌥ / click updates lastLookedAt", () => {
    let s = initialWindowsState();
    s = windowsReducer(s, open("tradingview", 1));
    s = windowsReducer(s, open("youtube", 2));
    s = windowsReducer(s, open("lease", 3));
    const tv = s.windows.find((w) => w.catalogId === "tradingview")!;
    s = windowsReducer(s, { type: "focus", id: tv.id, at: 9 });
    const slots = layoutSlots(s.windows, s.focusedId);
    expect(slots.find((x) => x.slot === "primary")?.id).toBe(tv.id);
  });
});

describe("voice targets", () => {
  it("resolves close/focus by kind or site", () => {
    let s = initialWindowsState();
    s = windowsReducer(s, open("tradingview", 1));
    s = windowsReducer(s, open("youtube", 2));
    const tv = s.windows.find((w) => w.catalogId === "tradingview")!;
    const yt = s.windows.find((w) => w.catalogId === "youtube")!;
    s = windowsReducer(s, { type: "focus", id: yt.id, at: 3 });
    expect(resolveVoiceTarget(s, "close it")).toEqual({ type: "close", target: "focused" });
    expect(resolveVoiceTarget(s, "close the chart")).toEqual({ type: "close", target: tv.id });
    expect(resolveVoiceTarget(s, "close the video")).toEqual({ type: "close", target: yt.id });
    expect(resolveVoiceTarget(s, "make the chart big")).toEqual({ type: "focus", id: tv.id });
    expect(resolveVoiceTarget(s, "close everything")).toEqual({ type: "close", target: "all" });
  });
});
