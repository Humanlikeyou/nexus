import { useLayoutEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type ThemePreference = "system" | "light" | "dark";

const THEME_KEY = "nexus.appearance.theme";
const MOTION_KEY = "nexus.appearance.reduceMotion";

function storedTheme(): ThemePreference {
  const value = window.localStorage.getItem(THEME_KEY);
  return value === "light" || value === "dark" ? value : "system";
}

/**
 * Match the native window appearance to the app theme. The sidebar's NSVisualEffectView follows
 * the window's appearance, not CSS: an app pinned to Light on a Dark Mac drew the light sidebar
 * tint over the dark material (mid-gray sidebar). "system" hands it back to macOS.
 */
export function syncWindowTheme(theme: ThemePreference): void {
  if (!("__TAURI_INTERNALS__" in window)) return;
  try {
    getCurrentWindow().setTheme(theme === "system" ? null : theme).catch(() => { /* CSS theme still applies */ });
  } catch {
    /* not running inside Tauri */
  }
}

export function useAppearance() {
  const [theme, setTheme] = useState<ThemePreference>(storedTheme);
  const [reduceMotion, setReduceMotion] = useState(() => window.localStorage.getItem(MOTION_KEY) === "true");

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem(THEME_KEY, theme);
    syncWindowTheme(theme);
  }, [theme]);

  useLayoutEffect(() => {
    document.documentElement.dataset.reduceMotion = String(reduceMotion);
    window.localStorage.setItem(MOTION_KEY, String(reduceMotion));
  }, [reduceMotion]);

  return { theme, setTheme, reduceMotion, setReduceMotion };
}
