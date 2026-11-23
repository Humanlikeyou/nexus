import { useSyncExternalStore } from "react";

/**
 * What the chat is doing, for the Home sand lens. Voice state comes from the voice bridge; text chat has no
 * such feed, so the chat screen publishes here: "thinking" while a reply is awaited, "idle" otherwise.
 * ("speaking" is accepted for a future streaming feed; the lens only maps "thinking".) No content is stored.
 */
export type AmaraActivity = "idle" | "thinking" | "speaking";

let current: AmaraActivity = "idle";
const listeners = new Set<() => void>();

export function getAmaraActivity(): AmaraActivity { return current; }
export function setAmaraActivity(next: AmaraActivity) {
  if (next === current) return;
  current = next;
  listeners.forEach((l) => l());
}
export function subscribeAmaraActivity(l: () => void) { listeners.add(l); return () => { listeners.delete(l); }; }
export function useAmaraActivity(): AmaraActivity {
  return useSyncExternalStore(subscribeAmaraActivity, getAmaraActivity, () => "idle" as AmaraActivity);
}
