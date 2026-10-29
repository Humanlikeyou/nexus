/**
 * Native sidebar vibrancy marker (screen-specs › Shell › Sidebar).
 * The main window is transparent with Tauri windowEffects ["sidebar"], which only exist on
 * macOS. There `<html data-vibrancy="true">` lets shell.css clear the window background under
 * the sidebar so the NSVisualEffectView shows through, while the content pane stays opaque.
 * Elsewhere (vite dev in a plain browser, other platforms) the attribute is absent and the
 * opaque window with the CSS backdrop-filter fallback applies.
 * WKWebView only: the e2e Tauri mock runs in Chromium on a Mac, where there is no native material
 * behind the page and a transparent body showed the white canvas under the dark sidebar tint.
 */
export function installVibrancy(root: HTMLElement = document.documentElement): void {
  const ua = navigator.userAgent;
  const wkWebView = /Macintosh/.test(ua) && /AppleWebKit/.test(ua) && !/Chrome|Chromium|Firefox|Edg\//.test(ua);
  if ("__TAURI_INTERNALS__" in window && wkWebView) root.dataset.vibrancy = "true";
  else delete root.dataset.vibrancy;
}
