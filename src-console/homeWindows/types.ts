/** design-1111w: Jarvis windows on Home. UI types only. */
export const MAX_WINDOWS = 3;
export const UNDO_MS = 8_000;

export type WindowKind = "web" | "video" | "chart" | "pdf" | "social";
export type OpenSource = "voice" | "chat";
export type ContentState = "loading" | "ready" | "signed_out" | "signing_in" | "blocked" | "offline" | "unwired";
export type StagePhase = "home" | "hearing" | "understood" | "exiting" | "entering" | "windows" | "closing" | "returning";
export type Signed = "in" | "out" | null;
export type CommentPhase = "off" | "reading" | "ready" | "paused";

export type OpenRequest = {
  catalogId: string;
  kind: WindowKind;
  title: string;
  source: string;
  siteId: string;
  fav: { color: string; letter: string };
  signed: Signed;
  chip: string;
  query: string;
  openedBy: OpenSource;
  target: { url?: string; query?: string; localPath?: string };
};

export type HomeWindow = {
  id: string;
  catalogId: string;
  kind: WindowKind;
  title: string;
  source: string;
  siteId: string;
  fav: { color: string; letter: string };
  signed: Signed;
  openedBy: OpenSource;
  openedAt: number;
  lastLookedAt: number;
  poppedOut: boolean;
  expanded: boolean;
  contentState: ContentState;
  comment: { phase: CommentPhase; text?: string };
  target: { url?: string; query?: string; localPath?: string };
};

export type ReplaceToast = {
  closedId: string;
  closedTitle: string;
  openedTitle: string;
  until: number;
};

export type HomeWindowsState = {
  phase: StagePhase;
  heard: { query: string; chip?: string; openedBy: OpenSource } | null;
  pending: OpenRequest | null;
  windows: HomeWindow[];
  focusedId: string | null;
  closingId: string | null;
  toast: ReplaceToast | null;
  /** Windows removed by the last close or replace, for Undo / ⌘Z within 8 s. */
  undo: { windows: HomeWindow[]; focusedId: string | null; openedId?: string; until: number } | null;
};

export const initialWindowsState = (): HomeWindowsState => ({
  phase: "home",
  heard: null,
  pending: null,
  windows: [],
  focusedId: null,
  closingId: null,
  toast: null,
  undo: null,
});
