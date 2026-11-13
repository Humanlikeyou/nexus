import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SECRET_PATTERN, homeWindowsAdapter, liveEmbedding, resetLiveSites } from "./adapters";
import { requestFromCatalog } from "./catalog";
import { liveEligible, startLiveSync, webviewVisible, type LiveIO, type LiveOpenResult } from "./live";
import { classifyOpenError, createLiveIO, REFUSED_FALLBACK } from "./liveIO";
import { windowsReducer, resetWindowIds, type WindowsAction } from "./reducer";
import { initialWindowsState, type ContentState, type HomeWindowsState, type OpenSource } from "./types";

function rig(open: (id: string) => LiveOpenResult = () => ({ ok: true, replaced: null })) {
  let state: HomeWindowsState = initialWindowsState();
  const subs = new Set<() => void>();
  const calls: string[] = [];
  const layouts: { id: string; visible: boolean }[][] = [];
  const io: LiveIO = {
    open: async ({ id, kind, url }) => { calls.push(`open ${id} ${kind} ${url}`); return open(id); },
    close: async (id) => { calls.push(`close ${id}`); },
    focus: async (id) => { calls.push(`focus ${id}`); },
    popOut: async (id) => { calls.push(`pop ${id}`); return true; },
    layout: async (r) => { layouts.push(r.map((x) => ({ id: x.id, visible: x.visible }))); },
  };
  const dispatch = (a: WindowsAction) => { state = windowsReducer(state, a); subs.forEach((l) => l()); };
  const sync = startLiveSync({
    io, get: () => state, subscribe: (l) => { subs.add(l); return () => subs.delete(l); }, dispatch,
    measure: () => ({ x: 10, y: 20, width: 300, height: 200 }), settleMs: () => 100, now: () => 1_000,
  });
  const openWin = (id: string, state_: ContentState = "loading", by: OpenSource = "chat", at = 1) =>
    dispatch({ type: "open", request: requestFromCatalog(id, by), at, contentState: state_ });
  return { sync, dispatch, calls, layouts, get: () => state, openWin };
}

describe("live Home windows sync", () => {
  beforeEach(() => { vi.useFakeTimers(); resetWindowIds(); });
  afterEach(() => { vi.useRealTimers(); });

  it("opens a new window through home_window_open with the id, kind and url, once", async () => {
    const r = rig();
    r.openWin("tradingview");
    r.dispatch({ type: "focus", id: r.get().windows[0].id, at: 2 });
    await r.sync.flush();
    expect(r.calls.filter((c) => c.startsWith("open"))).toEqual(["open win-tradingview-1 chart https://tradingview.com"]);
  });

  it("never opens the sample-only news host or a local PDF: they stay Needs wiring", async () => {
    const r = rig();
    r.openWin("akiya");
    r.openWin("lease", "loading", "chat", 2);
    await r.sync.flush();
    expect(r.calls.filter((c) => c.startsWith("open"))).toEqual([]);
    expect(r.get().windows.map((w) => w.contentState)).toEqual(["unwired", "unwired"]);
    expect(liveEligible(r.get().windows[0])).toBe(false);
  });

  it("closes the native window when the stage drops it (X, Esc, close all, last close)", async () => {
    const r = rig();
    r.openWin("tradingview");
    r.openWin("youtube", "loading", "chat", 2);
    await r.sync.flush();
    r.dispatch({ type: "close", target: "win-youtube-2", at: 3 });
    r.dispatch({ type: "close", target: "all", at: 4 });
    await r.sync.flush();
    expect(r.calls).toContain("close win-youtube-2");
    expect(r.calls).toContain("close win-tradingview-1");
  });

  it("follows the backend when it replaced a window the stage still shows", async () => {
    const r = rig((id) => ({ ok: true, replaced: id === "win-instagram-3" ? "win-youtube-2" : null }));
    r.openWin("tradingview", "loading", "chat", 1);
    r.openWin("youtube", "loading", "chat", 2);
    r.openWin("instagram", "loading", "chat", 3);
    await r.sync.flush();
    expect(r.get().windows.map((w) => w.id)).toEqual(["win-tradingview-1", "win-instagram-3"]);
  });

  it("an open that fails marks the window blocked (pop-out offered), unsupported marks it unwired", async () => {
    const r = rig(() => ({ ok: false, reason: "failed", copy: "x" }));
    r.openWin("youtube");
    await r.sync.flush();
    expect(r.get().windows[0].contentState).toBe("blocked");
    const u = rig(() => ({ ok: false, reason: "unsupported", copy: "x" }));
    u.openWin("youtube");
    await u.sync.flush();
    expect(u.get().windows[0].contentState).toBe("unwired");
  });

  it("hides the native view while the stage moves and shows it at rest, only for a ready or sign-in page", async () => {
    const r = rig();
    r.openWin("tradingview");
    await r.sync.flush();
    r.sync.onBackendState({ id: "win-tradingview-1", state: "ready", signedIn: false });
    await r.sync.flush();
    expect(r.layouts[r.layouts.length - 1]).toEqual([{ id: "win-tradingview-1", visible: false }]);
    vi.advanceTimersByTime(150);
    await r.sync.flush();
    expect(r.layouts[r.layouts.length - 1]).toEqual([{ id: "win-tradingview-1", visible: true }]);
    r.dispatch({ type: "setContent", id: "win-tradingview-1", contentState: "blocked" });
    vi.advanceTimersByTime(150);
    await r.sync.flush();
    expect(r.layouts[r.layouts.length - 1]).toEqual([{ id: "win-tradingview-1", visible: false }]);
    expect(webviewVisible(r.get(), r.get().windows[0])).toBe(false);
  });

  it("a sign-in page shows the page itself, marks signed out and pauses comments", async () => {
    const r = rig();
    r.openWin("instagram");
    await r.sync.flush();
    r.sync.onBackendState({ id: "win-instagram-1", state: "signing_in" });
    const w = r.get().windows[0];
    expect(w.contentState).toBe("signing_in");
    expect(w.signed).toBe("out");
    expect(w.comment.phase).toBe("paused");
    vi.advanceTimersByTime(150);
    await r.sync.flush();
    expect(r.layouts[r.layouts.length - 1]?.[0].visible).toBe(true);
  });

  it("a ready live page has no comment: they are not wired, nothing is invented", async () => {
    const r = rig();
    r.openWin("tradingview");
    r.sync.onBackendState({ id: "win-tradingview-1", state: "ready", signedIn: true });
    const w = r.get().windows[0];
    expect(w.comment.phase).toBe("off");
    expect(w.signed).toBe("in");
  });

  it("a pop-out window closed natively closes the window and returns Home when it was the last", async () => {
    const r = rig();
    r.openWin("tradingview");
    r.dispatch({ type: "popOut", id: "win-tradingview-1", at: 5 });
    await r.sync.flush();
    expect(r.calls).toContain("pop win-tradingview-1");
    r.sync.onBackendClosed("win-tradingview-1");
    expect(r.get().windows).toHaveLength(0);
    expect(r.get().phase).toBe("returning");
  });

  it("stop hides every native view (leaving Home)", async () => {
    const r = rig();
    r.openWin("tradingview");
    r.sync.stop();
    await Promise.resolve();
    expect(r.layouts[r.layouts.length - 1]).toEqual([]);
  });
});

describe("live Tauri IO and adapter", () => {
  afterEach(() => { delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; resetLiveSites(); localStorage.removeItem("nexus.sampleAdapters"); });

  it("refusals carry fixed copy and a failure never leaks the address", async () => {
    expect(classifyOpenError("refused: Web windows are off.")).toEqual({ ok: false, reason: "refused", copy: "Web windows are off." });
    expect(classifyOpenError("refused:").copy).toBe(REFUSED_FALLBACK);
    expect(classifyOpenError("unsupported: pdf").reason).toBe("unsupported");
    const io = createLiveIO(async () => { throw "failed: could not open example.com"; });
    const r = await io.open({ id: "w1", kind: "web", url: "https://example.com/?token=abc" });
    expect(r).toEqual({ ok: false, reason: "failed", copy: "could not open example.com" });
    expect(await io.preflight()).toEqual({ ok: false, copy: "could not open example.com" });
  });

  it("open sends only id, kind and the url as the target", async () => {
    const call = vi.fn(async () => ({ replaced: "old" }));
    const r = await createLiveIO(call as never).open({ id: "w1", kind: "chart", url: "https://tradingview.com" });
    expect(call).toHaveBeenCalledWith("home_window_open", { id: "w1", kind: "chart", target: { url: "https://tradingview.com" } });
    expect(r).toEqual({ ok: true, replaced: "old" });
  });

  it("in the app the adapter is live: sites are status only, sign out calls the backend, a decline throws", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const rows = [{ id: "instagram_com", host: "instagram.com", label: "Instagram", signed: "in" as const }];
    let declined = false;
    const calls: string[] = [];
    resetLiveSites(createLiveIO(async (cmd: string) => {
      calls.push(cmd);
      if (cmd === "home_window_sites") return (declined ? rows : calls.includes("home_window_sign_out") ? [] : rows) as never;
      if (cmd === "home_window_sign_out" && declined) throw "declined: Denied in the native approval dialog.";
      return undefined as never;
    }));
    const a = homeWindowsAdapter();
    expect(liveEmbedding()).toBe(true);
    expect(a.embedding().state).toBe("live");
    await a.refreshSites?.();
    const sites = a.signedSites();
    expect(sites.state).toBe("live");
    expect(JSON.stringify(sites)).not.toMatch(SECRET_PATTERN);
    expect((await a.signOut("instagram_com")).state).toBe("live");
    expect(calls).toContain("home_window_sign_out");
    declined = true;
    await expect(a.signOut("instagram_com")).rejects.toMatch(/declined/);
    expect((await a.comment({ siteId: "x", title: "y" })).state).toBe("unwired");
  });

  it("outside the app (browser, e2e) nothing is live", () => {
    expect(liveEmbedding()).toBe(false);
    expect(homeWindowsAdapter().embedding().state).toBe("unwired");
  });
});
