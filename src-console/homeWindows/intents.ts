/** Voice and typed commands for an open stage. Close / focus / expand stay on this Mac. */
import type { HomeWindow, HomeWindowsState } from "./types";
import { shortTitle } from "./reducer";

export type VoiceIntent =
  | { type: "close"; target: string | "focused" | "all" }
  | { type: "focus"; id: string }
  | { type: "expand"; id: string }
  | { type: "undo" }
  | { type: "open" };

const KIND: Record<string, (w: HomeWindow) => boolean> = {
  chart: (w) => w.kind === "chart",
  video: (w) => w.kind === "video",
  pdf: (w) => w.kind === "pdf",
  instagram: (w) => /instagram/i.test(w.source) || w.kind === "social",
  tradingview: (w) => /tradingview/i.test(w.source),
  youtube: (w) => /youtube/i.test(w.source),
  article: (w) => w.kind === "web",
};

function findNamed(windows: HomeWindow[], text: string): HomeWindow | undefined {
  const lower = text.toLowerCase();
  for (const [name, pred] of Object.entries(KIND)) {
    if (lower.includes(name)) return windows.find(pred);
  }
  return windows.find((w) => lower.includes(shortTitle(w.title).toLowerCase()) || lower.includes(w.source.toLowerCase()) || lower.includes(w.title.toLowerCase()));
}

export function resolveVoiceTarget(state: HomeWindowsState, text: string): VoiceIntent | null {
  const q = text.trim();
  if (!q) return null;
  if (/^(undo|oops|bring it back)\b/i.test(q)) return { type: "undo" };
  if (/close everything|close all|close every window/i.test(q)) return { type: "close", target: "all" };
  if (/^(close it|close this|close the window)\b/i.test(q)) return { type: "close", target: "focused" };
  const named = findNamed(state.windows, q);
  if (/^close\b/i.test(q) && named) return { type: "close", target: named.id };
  if (/\b(make .+ big|focus|look at|show the)\b/i.test(q) && named) return { type: "focus", id: named.id };
  if (/\bexpand\b/i.test(q) && named) return { type: "expand", id: named.id };
  if (/\bexpand\b/i.test(q) && state.focusedId) return { type: "expand", id: state.focusedId };
  return null;
}

/** Whether Amara may write a comment on this window (never on sign-in, signed out, blocked, offline, or unwired). */
export function canComment(w: HomeWindow): boolean {
  return w.contentState === "ready" || w.contentState === "loading";
}

/** Speak the comment only when he asked by voice. Chat shows the caption only. */
export function shouldSpeakComment(w: HomeWindow): boolean {
  return canComment(w) && w.comment.phase === "ready" && w.openedBy === "voice";
}
