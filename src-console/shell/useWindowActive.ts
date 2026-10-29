import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Inactive-window setter (screen-specs › Shell › Inactive window, Decision 21).
 * Mirrors key-window state onto `<html data-window-active="true|false">` so CSS can dim the
 * toolbar wordmark and the selected sidebar icon the way macOS does for background windows.
 * Source: Tauri `onFocusChanged` (key-window changes), with DOM focus/blur as the fallback
 * for vite dev in a plain browser. Returns a cleanup that removes every listener.
 */
export function installWindowActive(root: HTMLElement = document.documentElement): () => void {
  const set = (active: boolean) => { root.dataset.windowActive = String(active); };
  set(document.hasFocus());

  const onFocus = () => set(true);
  const onBlur = () => set(false);
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);

  let disposed = false;
  let unlisten: (() => void) | undefined;
  if ("__TAURI_INTERNALS__" in window) {
    try {
      getCurrentWindow()
        .onFocusChanged(({ payload }) => set(payload))
        .then((fn) => { if (disposed) fn(); else unlisten = fn; })
        .catch(() => { /* DOM fallback stays in charge */ });
    } catch {
      /* not running inside Tauri: DOM fallback only */
    }
  }

  return () => {
    disposed = true;
    unlisten?.();
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", onBlur);
  };
}

/** Shell-mount hook: installs the setter once for the app's lifetime. */
export function useWindowActive(): void {
  useEffect(() => installWindowActive(), []);
}
