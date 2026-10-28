import { chromium, defineConfig } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { Theme } from "./test/e2e/fixtures/tauri-mock";

const PORT = Number(process.env.E2E_PORT || 4179);

/**
 * Use Playwright's own Chromium when it is installed (CI runs `playwright
 * install`). Otherwise fall back to the newest build already in the local
 * Playwright cache, so a machine with an older cached build needs no download.
 */
function chromiumExecutable(): string | undefined {
  if (process.env.E2E_CHROMIUM) return process.env.E2E_CHROMIUM;
  try {
    if (fs.existsSync(chromium.executablePath())) return undefined;
  } catch {
    /* not installed */
  }
  const cache = process.env.PLAYWRIGHT_BROWSERS_PATH
    || (process.platform === "darwin" ? path.join(os.homedir(), "Library/Caches/ms-playwright") : path.join(os.homedir(), ".cache/ms-playwright"));
  if (!fs.existsSync(cache)) return undefined;
  const newest = (re: RegExp) => fs.readdirSync(cache).filter((d) => re.test(d)).sort((a, b) => Number(a.split("-").pop()) - Number(b.split("-").pop())).pop();
  const shell = newest(/^chromium_headless_shell-\d+$/);
  if (shell) {
    const dir = path.join(cache, shell);
    const sub = fs.readdirSync(dir).find((d) => d.startsWith("chrome-headless-shell"));
    const bin = sub && path.join(dir, sub, "chrome-headless-shell");
    if (bin && fs.existsSync(bin)) return bin;
  }
  const full = newest(/^chromium-\d+$/);
  if (full) {
    const dir = path.join(cache, full);
    const sub = fs.readdirSync(dir).find((d) => d.startsWith("chrome-mac") || d.startsWith("chrome-linux"));
    if (sub && sub.startsWith("chrome-linux")) return path.join(dir, sub, "chrome");
    const app = sub && fs.readdirSync(path.join(dir, sub)).find((d) => d.endsWith(".app"));
    if (app) return path.join(dir, sub, app, "Contents/MacOS", app.replace(/\.app$/, ""));
  }
  return undefined;
}

const themes: Theme[] = ["light", "dark"];

export default defineConfig<{ theme: Theme }>({
  testDir: "test/e2e",
  testMatch: "**/*.spec.ts",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [["list"], ["github"]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    viewport: { width: 1440, height: 900 },
    timezoneId: "Asia/Tokyo",
    locale: "en-US",
    trace: "retain-on-failure",
    launchOptions: { executablePath: chromiumExecutable() },
  },
  projects: themes.map((theme) => ({ name: theme, use: { theme, colorScheme: theme } })),
  webServer: {
    command: `npx vite preview --port ${PORT} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
