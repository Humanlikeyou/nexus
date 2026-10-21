import { useLayoutEffect, useState } from "react";

export type ThemePreference = "system" | "light" | "dark";

const THEME_KEY = "nexus.appearance.theme";
const MOTION_KEY = "nexus.appearance.reduceMotion";

function storedTheme(): ThemePreference {
  const value = window.localStorage.getItem(THEME_KEY);
  return value === "light" || value === "dark" ? value : "system";
}

export function useAppearance() {
  const [theme, setTheme] = useState<ThemePreference>(storedTheme);
  const [reduceMotion, setReduceMotion] = useState(() => window.localStorage.getItem(MOTION_KEY) === "true");

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    window.localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  useLayoutEffect(() => {
    document.documentElement.dataset.reduceMotion = String(reduceMotion);
    window.localStorage.setItem(MOTION_KEY, String(reduceMotion));
  }, [reduceMotion]);

  return { theme, setTheme, reduceMotion, setReduceMotion };
}
