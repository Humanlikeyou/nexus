import { listen } from "@tauri-apps/api/event";
import { getReduceTransparency } from "../bridge";

export const REDUCE_TRANSPARENCY_EVENT = "reduce-transparency-changed";

/**
 * Follows the macOS Reduce Transparency setting: `<html data-reduce-transparency="true">` while
 * it's on, which turns on the solid fallbacks in CSS. WKWebView doesn't report this setting to
 * CSS, so Rust reads it at startup and re-checks on window focus. Returns a stop function.
 */
export function installReduceTransparency(root: HTMLElement = document.documentElement): () => void {
  if (!("__TAURI_INTERNALS__" in window)) return () => undefined;
  let stopped = false;
  let heard = false;
  let unlisten: (() => void) | null = null;
  const apply = (on: boolean) => {
    if (on) root.dataset.reduceTransparency = "true";
    else delete root.dataset.reduceTransparency;
  };
  void listen<boolean>(REDUCE_TRANSPARENCY_EVENT, ({ payload }) => {
    if (stopped) return;
    heard = true;
    apply(payload === true);
  }).then((cleanup) => { if (stopped) cleanup(); else unlisten = cleanup; })
    .catch((error) => console.warn("[appearance] reduce transparency listener unavailable", error));
  // An event that beat this read is newer than it. The attribute starts absent, so only "on" matters.
  void getReduceTransparency().then((on) => { if (!stopped && !heard && on === true) apply(true); })
    .catch((error) => console.warn("[appearance] reduce transparency unavailable", error));
  return () => { stopped = true; unlisten?.(); };
}
