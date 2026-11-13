/**
 * Live Home windows (HANDOFF-1111w §3, decision 0014 #5): the logic side of the native webview
 * backend (src-tauri commands/home_windows.rs). Logic only; markup stays with Design.
 *
 * A subscriber turns the stage store into backend calls: a window that appears is opened (the one
 * write path, `home_window_open`), one that disappears is closed, focus and pop-out follow, and the
 * measured slot rects are sent so native webviews sit over the stage at rest and are hidden while it
 * moves (a native webview can't be CSS-animated, so the UI's own body is the placeholder).
 * Nothing here reads page content, and URLs are never logged.
 */
import type { OpenKind } from "./liveTypes";
import type { HomeWindow, HomeWindowsState } from "./types";
import type { WindowsAction } from "./reducer";

export type LiveOpenResult =
  | { ok: true; replaced: string | null }
  | { ok: false; reason: "refused" | "unsupported" | "failed"; copy: string };

export type SlotRect = { id: string; x: number; y: number; width: number; height: number; visible: boolean };

export type LiveIO = {
  open(input: { id: string; kind: OpenKind; url: string }): Promise<LiveOpenResult>;
  close(id: string): Promise<void>;
  focus(id: string): Promise<void>;
  popOut(id: string): Promise<boolean>;
  layout(rects: SlotRect[]): Promise<void>;
};

/** The sample catalog's fake news host and local PDFs have no real page to open. */
export function liveEligible(w: Pick<HomeWindow, "kind" | "siteId" | "target">): boolean {
  return w.kind !== "pdf" && w.siteId !== "example-news" && /^https?:\/\//i.test(w.target.url ?? "");
}

/** Where a native webview may be visible: at rest, and the page itself is what the user should see. */
export function webviewVisible(state: HomeWindowsState, w: HomeWindow): boolean {
  return state.phase === "windows" && state.closingId !== w.id && !w.poppedOut
    && (w.contentState === "ready" || w.contentState === "signing_in");
}

export function layoutSignature(state: HomeWindowsState): string {
  return [state.phase, state.focusedId, ...state.windows.map((w) => `${w.id}:${w.expanded ? 1 : 0}:${w.poppedOut ? 1 : 0}:${w.contentState}`)].join("|");
}

type Deps = {
  io: LiveIO;
  get(): HomeWindowsState;
  subscribe(l: () => void): () => void;
  dispatch(a: WindowsAction): void;
  measure(id: string): { x: number; y: number; width: number; height: number } | null;
  settleMs(): number;
  now(): number;
};

export function startLiveSync(d: Deps): { stop(): void; onBackendState(p: { id: string; state: string; signedIn?: boolean | null }): void; onBackendClosed(id: string): void; flush(): Promise<void>; relayout(): void } {
  const known = new Set<string>();
  const popped = new Set<string>();
  const failed = new Set<string>(); // an open that failed is not retried on its own
  let lastFocus: string | null = null;
  let lastSig = "";
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  const pending: Promise<unknown>[] = [];
  const track = <T,>(p: Promise<T>): Promise<T> => { pending.push(p); return p; };

  const sendLayout = (hideAll: boolean) => {
    const s = d.get();
    const rects: SlotRect[] = [];
    for (const w of s.windows.slice(0, 3)) {
      if (!known.has(w.id)) continue;
      const r = d.measure(w.id);
      const visible = !hideAll && !!r && r.width >= 1 && r.height >= 1 && webviewVisible(s, w);
      rects.push({ id: w.id, x: r?.x ?? 0, y: r?.y ?? 0, width: r?.width ?? 0, height: r?.height ?? 0, visible });
    }
    void track(d.io.layout(rects).catch(() => undefined));
  };

  const schedule = () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => { if (!stopped) sendLayout(false); }, d.settleMs());
  };

  const sync = () => {
    if (stopped) return;
    const s = d.get();
    const ids = new Set(s.windows.map((w) => w.id));
    for (const id of [...failed]) if (!ids.has(id)) failed.delete(id);
    for (const id of [...known]) {
      if (!ids.has(id)) { known.delete(id); popped.delete(id); void track(d.io.close(id).catch(() => undefined)); }
    }
    for (const w of s.windows) {
      if (known.has(w.id) || failed.has(w.id)) continue;
      if (!liveEligible(w)) { if (w.contentState !== "unwired") d.dispatch({ type: "setContent", id: w.id, contentState: "unwired" }); continue; }
      known.add(w.id);
      void track(d.io.open({ id: w.id, kind: w.kind as OpenKind, url: w.target.url as string }).then((r) => {
        if (stopped) return;
        if (!r.ok) {
          known.delete(w.id);
          failed.add(w.id);
          d.dispatch({ type: "setContent", id: w.id, contentState: r.reason === "unsupported" ? "unwired" : "blocked" });
          return;
        }
        // The backend's own least-recently-looked choice wins if it differs from the stage's.
        if (r.replaced && d.get().windows.some((x) => x.id === r.replaced)) d.dispatch({ type: "close", target: r.replaced, at: d.now() });
      }).catch(() => { known.delete(w.id); failed.add(w.id); d.dispatch({ type: "setContent", id: w.id, contentState: "blocked" }); }));
    }
    if (s.focusedId && s.focusedId !== lastFocus && known.has(s.focusedId)) void track(d.io.focus(s.focusedId).catch(() => undefined));
    lastFocus = s.focusedId;
    for (const w of s.windows) {
      if (w.poppedOut && known.has(w.id) && !popped.has(w.id)) {
        popped.add(w.id);
        void track(d.io.popOut(w.id).then((ok) => { if (!ok) { popped.delete(w.id); d.dispatch({ type: "popIn", id: w.id, at: d.now() }); d.dispatch({ type: "setContent", id: w.id, contentState: "blocked" }); } }).catch(() => undefined));
      }
    }
    const sig = layoutSignature(s);
    if (sig !== lastSig) {
      lastSig = sig;
      // Moving: hide the native views, show the UI body; they come back once the stage is at rest.
      sendLayout(true);
      schedule();
    }
  };

  const unsub = d.subscribe(sync);
  sync();
  return {
    stop() {
      stopped = true;
      if (timer) clearTimeout(timer);
      unsub();
      void d.io.layout([]).catch(() => undefined); // leaving Home: nothing native stays on screen
    },
    onBackendState(p) {
      const w = d.get().windows.find((x) => x.id === p.id);
      if (!w) return;
      if (p.state === "signing_in") d.dispatch({ type: "setContent", id: p.id, contentState: "signing_in", signed: "out" });
      else if (p.state === "ready") {
        d.dispatch({ type: "setContent", id: p.id, contentState: "ready", signed: p.signedIn ? "in" : null });
        d.dispatch({ type: "commentOff", id: p.id }); // comments on live pages are not wired
      }
      schedule();
    },
    onBackendClosed(id) {
      popped.delete(id);
      if (d.get().windows.some((w) => w.id === id)) d.dispatch({ type: "close", target: id, at: d.now() });
    },
    relayout() { schedule(); },
    async flush() { await Promise.all(pending.splice(0)); },
  };
}
