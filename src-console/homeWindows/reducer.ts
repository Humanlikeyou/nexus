/** Pure reducer for Home windows. Pop-outs count toward the max of 3.
 *  A 4th window replaces the one looked at least recently (not the oldest opened). */
import type { ContentState, HomeWindow, HomeWindowsState, OpenRequest, Signed } from "./types";
import { MAX_WINDOWS, UNDO_MS, initialWindowsState } from "./types";

export type WindowsAction =
  | { type: "reset" }
  | { type: "hydrate"; state: HomeWindowsState }
  | { type: "hear"; query: string; openedBy: OpenRequest["openedBy"] }
  | { type: "understand"; request: OpenRequest }
  | { type: "beginExit" }
  | { type: "beginEnter" }
  | { type: "open"; request: OpenRequest; at: number; contentState?: ContentState }
  | { type: "commentReading"; id: string }
  | { type: "commentReady"; id: string; text: string }
  | { type: "focus"; id: string; at: number }
  | { type: "close"; target: string | "focused" | "all"; at: number }
  | { type: "closeFinished" }
  | { type: "returnFinished" }
  | { type: "expand"; id: string; at: number }
  | { type: "restore" }
  | { type: "popOut"; id: string; at: number }
  | { type: "popIn"; id: string; at: number }
  | { type: "setContent"; id: string; contentState: ContentState; signed?: Signed }
  | { type: "undo"; at: number }
  | { type: "clearToast" };

let seq = 0;
export const resetWindowIds = (): void => { seq = 0; };
const nextId = (catalogId: string): string => `win-${catalogId}-${++seq}`;

export const stageWindows = (windows: HomeWindow[]): HomeWindow[] => windows.filter((w) => !w.poppedOut);
export const countTowardCap = (windows: HomeWindow[]): number => windows.length;

export function lruId(windows: HomeWindow[]): string {
  return windows.reduce((best, w) => {
    if (w.lastLookedAt < best.lastLookedAt) return w;
    if (w.lastLookedAt === best.lastLookedAt && w.openedAt < best.openedAt) return w;
    return best;
  }).id;
}

export function layoutSlots(windows: HomeWindow[], focusedId: string | null): { id: string; slot: "primary" | "a" | "b" }[] {
  const on = stageWindows(windows);
  if (on.length === 0) return [];
  const focused = on.find((w) => w.id === focusedId) ?? on[on.length - 1];
  const rest = on.filter((w) => w.id !== focused.id);
  if (on.length === 1) return [{ id: focused.id, slot: "primary" }];
  if (on.length === 2) return [{ id: focused.id, slot: "primary" }, { id: rest[0].id, slot: "a" }];
  // 3 on stage: primary + right column 1.618 (a) : 1 (b). Older of the two small ones goes to b.
  const [newer, older] = rest[0].lastLookedAt >= rest[1].lastLookedAt ? [rest[0], rest[1]] : [rest[1], rest[0]];
  return [{ id: focused.id, slot: "primary" }, { id: newer.id, slot: "a" }, { id: older.id, slot: "b" }];
}

function makeWindow(request: OpenRequest, at: number, contentState: ContentState): HomeWindow {
  const signing = contentState === "signing_in" || contentState === "signed_out";
  return {
    id: nextId(request.catalogId),
    catalogId: request.catalogId,
    kind: request.kind,
    title: request.title,
    source: request.source,
    siteId: request.siteId,
    fav: request.fav,
    signed: request.signed,
    openedBy: request.openedBy,
    openedAt: at,
    lastLookedAt: at,
    poppedOut: false,
    expanded: false,
    contentState,
    comment: { phase: signing || contentState === "blocked" || contentState === "offline" || contentState === "unwired" || contentState === "loading" ? (contentState === "loading" ? "reading" : "paused") : "off" },
    target: request.target,
  };
}

function look(w: HomeWindow, at: number, id: string): HomeWindow {
  return w.id === id ? { ...w, lastLookedAt: at, expanded: w.expanded } : w;
}

export function windowsReducer(state: HomeWindowsState, action: WindowsAction): HomeWindowsState {
  switch (action.type) {
    case "reset":
      return initialWindowsState();
    case "hydrate":
      return action.state;
    case "hear":
      return { ...state, phase: "hearing", heard: { query: action.query, openedBy: action.openedBy }, pending: null };
    case "understand":
      return { ...state, phase: "understood", heard: { query: action.request.query, chip: action.request.chip, openedBy: action.request.openedBy }, pending: action.request };
    case "beginExit":
      return { ...state, phase: "exiting" };
    case "beginEnter":
      return { ...state, phase: "entering" };
    case "open": {
      const incoming = makeWindow(action.request, action.at, action.contentState ?? "loading");
      if (state.windows.some((w) => w.catalogId === incoming.catalogId && !w.poppedOut)) {
        // Already on the stage: focus it instead of opening a duplicate.
        const existing = state.windows.find((w) => w.catalogId === incoming.catalogId)!;
        return {
          ...state,
          phase: "windows",
          heard: null,
          pending: null,
          focusedId: existing.id,
          windows: state.windows.map((w) => look({ ...w, expanded: false }, action.at, existing.id)),
        };
      }
      if (state.windows.length < MAX_WINDOWS) {
        return {
          ...state,
          phase: "windows",
          heard: null,
          pending: null,
          focusedId: incoming.id,
          windows: [...state.windows.map((w) => ({ ...w, expanded: false })), incoming],
          closingId: null,
        };
      }
      const evictId = lruId(state.windows);
      const evicted = state.windows.find((w) => w.id === evictId)!;
      return {
        ...state,
        phase: "windows",
        heard: null,
        pending: null,
        focusedId: incoming.id,
        windows: [...state.windows.filter((w) => w.id !== evictId).map((w) => ({ ...w, expanded: false })), incoming],
        toast: { closedId: evictId, closedTitle: evicted.title, openedTitle: incoming.title, until: action.at + UNDO_MS },
        undo: { windows: [evicted], focusedId: state.focusedId, openedId: incoming.id, until: action.at + UNDO_MS },
        closingId: null,
      };
    }
    case "commentReading":
      return { ...state, windows: state.windows.map((w) => w.id === action.id ? { ...w, comment: { phase: "reading" } } : w) };
    case "commentReady":
      return { ...state, windows: state.windows.map((w) => w.id === action.id ? { ...w, comment: { phase: "ready", text: action.text } } : w) };
    case "focus":
      return {
        ...state,
        focusedId: action.id,
        windows: state.windows.map((w) => look(w, action.at, action.id)),
      };
    case "close": {
      const ids = action.target === "all" ? state.windows.map((w) => w.id)
        : action.target === "focused" ? (state.focusedId ? [state.focusedId] : [])
        : [action.target];
      const removed = state.windows.filter((w) => ids.includes(w.id));
      if (removed.length === 0) return state;
      const remaining = state.windows.filter((w) => !ids.includes(w.id));
      const last = remaining.length === 0;
      const nextFocus = remaining.find((w) => w.id === state.focusedId)?.id
        ?? remaining.slice().sort((a, b) => b.lastLookedAt - a.lastLookedAt)[0]?.id
        ?? null;
      return {
        ...state,
        phase: last ? "returning" : "closing",
        closingId: last ? null : (ids[0] ?? null),
        windows: remaining.map((w) => ({ ...w, expanded: false })),
        focusedId: nextFocus,
        undo: { windows: removed, focusedId: state.focusedId, until: action.at + UNDO_MS },
        toast: null,
        heard: null,
        pending: null,
      };
    }
    case "closeFinished":
      return state.phase === "closing" ? { ...state, phase: "windows", closingId: null } : state;
    case "returnFinished":
      return { ...initialWindowsState(), undo: state.undo };
    case "expand":
      return {
        ...state,
        focusedId: action.id,
        windows: state.windows.map((w) => ({ ...look(w, action.at, action.id), expanded: w.id === action.id ? !w.expanded : false })),
      };
    case "restore":
      return { ...state, windows: state.windows.map((w) => ({ ...w, expanded: false })) };
    case "popOut":
      return {
        ...state,
        focusedId: action.id,
        windows: state.windows.map((w) => w.id === action.id ? { ...w, poppedOut: true, expanded: false, lastLookedAt: action.at } : { ...w, expanded: false }),
      };
    case "popIn":
      return {
        ...state,
        focusedId: action.id,
        windows: state.windows.map((w) => w.id === action.id ? { ...w, poppedOut: false, lastLookedAt: action.at } : w),
      };
    case "setContent":
      return {
        ...state,
        windows: state.windows.map((w) => {
          if (w.id !== action.id) return w;
          const contentState = action.contentState;
          const quiet = contentState === "signed_out" || contentState === "signing_in" || contentState === "blocked" || contentState === "offline" || contentState === "unwired";
          return {
            ...w,
            contentState,
            signed: action.signed !== undefined ? action.signed : w.signed,
            comment: quiet ? { phase: "paused" } : contentState === "loading" ? { phase: "reading" } : w.comment,
          };
        }),
      };
    case "undo": {
      if (!state.undo || action.at > state.undo.until) return { ...state, undo: null, toast: null };
      const pack = state.undo;
      // Replace-undo: remove the window that took its place, restore the evicted one.
      const withoutOpened = pack.openedId ? state.windows.filter((w) => w.id !== pack.openedId) : state.windows;
      const restored = pack.windows.map((w) => ({ ...w, expanded: false, lastLookedAt: action.at }));
      const ids = new Set(restored.map((w) => w.id));
      const windows = [...withoutOpened.filter((w) => !ids.has(w.id)), ...restored];
      return {
        ...state,
        phase: windows.length ? "windows" : "home",
        windows,
        focusedId: pack.focusedId && windows.some((w) => w.id === pack.focusedId) ? pack.focusedId : restored[0]?.id ?? null,
        undo: null,
        toast: null,
        closingId: null,
        heard: null,
        pending: null,
      };
    }
    case "clearToast":
      return { ...state, toast: null };
    default:
      return state;
  }
}

export const toastLabel = (t: { closedTitle: string; openedTitle: string }): string =>
  `Closed ${shortTitle(t.closedTitle)} (looked at least recently) to make room for ${shortTitle(t.openedTitle)}`;

export const shortTitle = (title: string): string => {
  if (/tradingview|btcusd/i.test(title)) return "TradingView";
  if (/instagram/i.test(title)) return "Instagram";
  if (/youtube|label approval/i.test(title)) return "YouTube";
  if (/\.pdf/i.test(title)) return "the PDF";
  return title.split("·")[0].trim();
};
