/** Controller: prompt → hearing → understood → exit → open, plus preview flags for e2e/captures. */
import { useEffect, useRef } from "react";
import { sampleAdaptersEnabled } from "../lib/sampleAdapters";
import { entryById, parseWindowPrompt, requestFromCatalog } from "./catalog";
import { canComment, resolveVoiceTarget } from "./intents";
import { homeWindowsAdapter } from "./adapters";
import { homeWindowsStore } from "./store";
import { windowsReducer, resetWindowIds, layoutSlots } from "./reducer";
import type { ContentState, HomeWindowsState, OpenRequest, OpenSource, Signed } from "./types";
import { initialWindowsState } from "./types";

export const PREVIEW_KEY = "nexus.homeWindowsPreview";

export type PreviewScenario =
  | "hearing" | "understood" | "exiting" | "entering" | "reading" | "spoken" | "chat-caption"
  | "two" | "three" | "fourth" | "instagram-signed-out" | "instagram-sign-in"
  | "blocked" | "offline" | "web" | "expanded" | "closing" | "returning" | "home";

export type Preview = { scenario: PreviewScenario; openedBy?: OpenSource };

const reduced = () => typeof document !== "undefined" && (document.documentElement.dataset.reduceMotion === "true"
  || (typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches));

const now = () => Date.now();
const dispatch = homeWindowsStore.dispatch.bind(homeWindowsStore);

function tick(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, reduced() ? Math.min(ms, 150) : ms));
}

export function readPreview(): Preview | null {
  try {
    const raw = globalThis.localStorage?.getItem(PREVIEW_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as Preview;
  } catch { return null; }
}

function buildPreview(p: Preview): HomeWindowsState {
  resetWindowIds();
  const by = p.openedBy ?? (p.scenario === "chat-caption" ? "chat" : "voice");
  const t0 = Date.now();
  const open = (id: string, at: number, src: OpenSource = by, state: ContentState = "ready", signed?: Signed) =>
    ({ type: "open" as const, request: requestFromCatalog(id, src), at, contentState: state, ...(signed ? {} : {}) });
  const comment = (s: HomeWindowsState, id: string, text: string) => windowsReducer(s, { type: "commentReady", id: s.windows.find((w) => w.catalogId === id)!.id, text });
  const ready = (ids: string[], at0: number = t0) => {
    let s = initialWindowsState();
    ids.forEach((id, i) => { s = windowsReducer(s, open(id, at0 + i, by, "ready")); });
    ids.forEach((id) => { s = comment(s, id, entryById(id).comment); });
    return s;
  };
  switch (p.scenario) {
    case "home":
      return initialWindowsState();
    case "hearing":
      return windowsReducer(initialWindowsState(), { type: "hear", query: "pull up bitcoin on TradingView", openedBy: by });
    case "understood": {
      const request = requestFromCatalog("tradingview", by, "pull up bitcoin on TradingView");
      const s = windowsReducer(initialWindowsState(), { type: "hear", query: request.query, openedBy: by });
      return windowsReducer(s, { type: "understand", request });
    }
    case "exiting": {
      const request = requestFromCatalog("tradingview", by, "pull up bitcoin on TradingView");
      const s = windowsReducer(initialWindowsState(), { type: "understand", request });
      return windowsReducer(s, { type: "beginExit" });
    }
    case "entering": {
      let s = ready(["tradingview"]);
      s = windowsReducer(s, { type: "commentReading", id: s.windows[0].id });
      return { ...s, phase: "entering", windows: s.windows.map((w) => ({ ...w, contentState: "loading", comment: { phase: "reading" } })) };
    }
    case "reading": {
      const s = ready(["tradingview"]);
      return windowsReducer({ ...s, windows: s.windows.map((w) => ({ ...w, contentState: "ready" })) }, { type: "commentReading", id: s.windows[0].id });
    }
    case "spoken":
    case "chat-caption":
      return ready(["tradingview"]);
    case "two":
      return ready(["tradingview", "youtube"]);
    case "three":
      return ready(["tradingview", "youtube", "lease"]);
    case "fourth":
      return windowsReducer(ready(["tradingview", "youtube", "lease"]), open("instagram", t0 + 10, by, "ready"));
    case "instagram-signed-out": {
      const s = windowsReducer(initialWindowsState(), open("instagram", t0, by, "signed_out"));
      return windowsReducer(s, { type: "setContent", id: s.windows[0].id, contentState: "signed_out", signed: "out" });
    }
    case "instagram-sign-in": {
      const s = windowsReducer(initialWindowsState(), open("instagram", t0, by, "signing_in"));
      return windowsReducer(s, { type: "setContent", id: s.windows[0].id, contentState: "signing_in", signed: "out" });
    }
    case "blocked":
      return windowsReducer(initialWindowsState(), open("tradingview", t0, by, "blocked"));
    case "offline":
      return windowsReducer(initialWindowsState(), open("akiya", t0, by, "offline"));
    case "web":
      return ready(["akiya"]);
    case "expanded": {
      const s = ready(["tradingview", "youtube", "lease"]);
      const pdf = s.windows.find((w) => w.catalogId === "lease")!;
      return windowsReducer(s, { type: "expand", id: pdf.id, at: t0 + 20 });
    }
    case "closing": {
      const s = ready(["tradingview", "youtube", "lease"]);
      const yt = s.windows.find((w) => w.catalogId === "youtube")!;
      return windowsReducer(s, { type: "close", target: yt.id, at: t0 + 20 });
    }
    case "returning": {
      let s = ready(["tradingview"]);
      s = windowsReducer(s, { type: "close", target: s.windows[0].id, at: t0 + 20 });
      return s;
    }
    default:
      return initialWindowsState();
  }
}

export function applyPreview(p: Preview): void {
  homeWindowsStore.reset();
  resetWindowIds();
  dispatch({ type: "hydrate", state: buildPreview(p) });
}

async function animateOpen(request: OpenRequest): Promise<void> {
  dispatch({ type: "hear", query: request.query, openedBy: request.openedBy });
  await tick(400);
  dispatch({ type: "understand", request });
  await tick(700);
  dispatch({ type: "beginExit" });
  await tick(reduced() ? 150 : 320);
  dispatch({ type: "beginEnter" });
  const sample = sampleAdaptersEnabled();
  const contentState: ContentState = sample ? "loading" : "unwired";
  dispatch({ type: "open", request, at: now(), contentState });
  const opened = homeWindowsStore.get().windows.find((w) => w.catalogId === request.catalogId);
  if (!opened) return;
  await tick(reduced() ? 150 : 320);
  if (!sample) return;
  dispatch({ type: "setContent", id: opened.id, contentState: "ready" });
  if (!canComment({ ...opened, contentState: "ready" })) return;
  dispatch({ type: "commentReading", id: opened.id });
  await tick(900);
  const text = entryById(request.catalogId).comment;
  dispatch({ type: "commentReady", id: opened.id, text });
  if (request.openedBy === "voice") void homeWindowsAdapter().speak(text);
}

/** Shared write path for chat and voice. Returns true when the prompt was a window request. */
export function handleWindowPrompt(text: string, openedBy: OpenSource): boolean {
  const intent = resolveVoiceTarget(homeWindowsStore.get(), text);
  if (intent) {
    applyIntent(intent);
    return true;
  }
  const request = parseWindowPrompt(text, openedBy);
  if (!request) return false;
  void animateOpen(request);
  return true;
}

export function applyIntent(intent: ReturnType<typeof resolveVoiceTarget>): void {
  if (!intent) return;
  const t = now();
  if (intent.type === "undo") dispatch({ type: "undo", at: t });
  else if (intent.type === "close") dispatch({ type: "close", target: intent.target, at: t });
  else if (intent.type === "focus") dispatch({ type: "focus", id: intent.id, at: t });
  else if (intent.type === "expand") dispatch({ type: "expand", id: intent.id, at: t });
}

export function closeFocused(): void { dispatch({ type: "close", target: "focused", at: now() }); }
export function closeAll(): void { dispatch({ type: "close", target: "all", at: now() }); }
export function undoWindows(): void { dispatch({ type: "undo", at: now() }); }
export function focusWindow(id: string): void { dispatch({ type: "focus", id, at: now() }); }
export function expandWindow(id: string): void { dispatch({ type: "expand", id, at: now() }); }
export function popOutWindow(id: string): void {
  if (!sampleAdaptersEnabled() && homeWindowsAdapter().embedding().state === "unwired") {
    dispatch({ type: "setContent", id, contentState: "unwired" });
    return;
  }
  dispatch({ type: "popOut", id, at: now() });
}
export function popInWindow(id: string): void { dispatch({ type: "popIn", id, at: now() }); }

/** Phase timers: finish close / return, expire Undo toast. */
export function useHomeWindowsClock(): void {
  const subscribe = homeWindowsStore.subscribe;
  useEffect(() => {
    const preview = readPreview();
    if (preview && sampleAdaptersEnabled()) applyPreview(preview);
    return () => { /* leave preview in place for the session */ };
  }, []);
  useEffect(() => {
    let timer: number | undefined;
    const arm = () => {
      if (timer) window.clearTimeout(timer);
      const s = homeWindowsStore.get();
      if (s.phase === "closing") timer = window.setTimeout(() => dispatch({ type: "closeFinished" }), reduced() ? 150 : 240);
      else if (s.phase === "returning") timer = window.setTimeout(() => dispatch({ type: "returnFinished" }), reduced() ? 150 : 320);
      else if (s.toast) timer = window.setTimeout(() => dispatch({ type: "clearToast" }), Math.max(0, s.toast.until - Date.now()));
      else if (s.undo && !s.toast) timer = window.setTimeout(() => { /* undo window expires silently */ }, Math.max(0, s.undo.until - Date.now()));
    };
    arm();
    return subscribe(() => arm());
  }, [subscribe]);
}

export function useWindowKeys(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = homeWindowsStore.get();
      if (s.phase === "home" || s.phase === "hearing" || s.phase === "understood") return;
      const cmd = e.metaKey || e.ctrlKey;
      if (e.key === "Escape" || (cmd && e.key.toLowerCase() === "w")) { e.preventDefault(); closeFocused(); return; }
      if (cmd && e.key.toLowerCase() === "z" && !e.shiftKey) { e.preventDefault(); undoWindows(); return; }
      if (e.altKey && ["1", "2", "3"].includes(e.key)) {
        e.preventDefault();
        const slots = layoutSlots(s.windows, s.focusedId);
        const i = Number(e.key) - 1;
        if (slots[i]) focusWindow(slots[i].id);
        return;
      }
      if (cmd && e.shiftKey && e.key === "Enter" && s.focusedId) { e.preventDefault(); expandWindow(s.focusedId); return; }
      if (cmd && e.shiftKey && e.key.toLowerCase() === "o" && s.focusedId) { e.preventDefault(); popOutWindow(s.focusedId); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/** Intercept new spoken turns after mount. The first poll is historical and is skipped. */
export function useVoiceWindowPrompts(turns: { time?: string; heard?: string; text?: string }[]): void {
  const primed = useRef(false);
  const seen = useRef("");
  useEffect(() => {
    const last = turns.length ? turns[turns.length - 1] : undefined;
    const key = last ? `${last.time ?? ""}:${last.heard ?? last.text ?? ""}` : "";
    if (!primed.current) {
      primed.current = true;
      seen.current = key;
      return;
    }
    if (!last || seen.current === key) return;
    seen.current = key;
    const q = (last.heard || last.text || "").trim();
    if (q) handleWindowPrompt(q, "voice");
  }, [turns]);
}
