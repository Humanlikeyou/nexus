/**
 * Native sidebar vibrancy marker (screen-specs › Shell › Sidebar).
 * The main window is transparent with Tauri windowEffects ["sidebar"], which only exist on
 * macOS. There `<html data-vibrancy="true">` lets shell.css clear the window background under
 * the sidebar so the NSVisualEffectView shows through, while the content pane stays opaque.
 * Elsewhere (vite dev in a plain browser, other platforms) the attribute is absent and the
 * opaque window with the CSS backdrop-filter fallback applies.
 */
export function installVibrancy(root: HTMLElement = document.documentElement): void {
  const mac = /Mac/i.test(navigator.userAgent);
  if ("__TAURI_INTERNALS__" in window && mac) root.dataset.vibrancy = "true";
  else delete root.dataset.vibrancy;
}
