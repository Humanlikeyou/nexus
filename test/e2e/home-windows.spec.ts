import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, routeSettled, test } from "./fixtures/tauri-mock";

/** design-1111w: Jarvis windows on Home (Taisun, 19:44 JST). */
const sample = (page: Page) => page.addInitScript(() => { localStorage.setItem("nexus.sampleAdapters", "1"); });

/** Apply a mid-frame preview after openApp, so the Home nav is visible for the fixture first. */
async function apply(page: Page, scenario: string, openedBy?: "voice" | "chat") {
  await page.evaluate(({ scenario, openedBy }) => {
    localStorage.setItem("nexus.sampleAdapters", "1");
    localStorage.setItem("nexus.homeWindowsPreview", JSON.stringify({ scenario, openedBy }));
  }, { scenario, openedBy });
  await page.reload();
  await expect(page.locator(".landing")).toBeVisible();
}

async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}

async function contrast(page: Page, sel: string) {
  return page.locator(sel).first().evaluate((el) => {
    const cs = getComputedStyle(el);
    return { colorScheme: cs.colorScheme, bg: cs.backgroundColor, color: cs.color };
  });
}

test("preview: one spoken window, axe clean in dark and light, windows stay dark in light theme", async ({ page, theme, consoleErrors }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await apply(page, "spoken", "voice");
  const stage = page.getByTestId("home-windows-stage");
  await expect(stage).toBeVisible();
  await expect(page.getByRole("region", { name: /TradingView window/ })).toBeVisible();
  await expect(stage.getByText("Amara:")).toBeVisible();
  await expect(stage.getByRole("button", { name: "Replay" })).toBeVisible();
  await expect(stage).toContainText("Windows");
  await expect(stage).toContainText("of 3");
  const win = await contrast(page, ".w-win");
  expect(win.colorScheme).toBe("dark");
  // Dark glass, not a light-theme wash.
  expect(win.bg).not.toMatch(/rgb\(\s*2[4-9]\d/);
  expect(await blocking(page)).toEqual([]);
  expect(consoleErrors).toEqual([]);
});

test("chat-opened comment is caption only: no Replay / Spoken", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await apply(page, "chat-caption", "chat");
  const stage = page.getByTestId("home-windows-stage");
  await expect(stage.getByText("Amara:")).toBeVisible();
  await expect(stage.getByRole("button", { name: "Replay" })).toHaveCount(0);
  await expect(stage.getByText("Spoken", { exact: true })).toHaveCount(0);
  expect(await blocking(page)).toEqual([]);
});

test("fourth window replaces the one looked at least recently, with an 8s Undo toast", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await apply(page, "fourth");
  const stage = page.getByTestId("home-windows-stage");
  await expect(stage.getByRole("region", { name: /Instagram window/ })).toBeVisible();
  await expect(stage.getByRole("status").filter({ hasText: /looked at least recently/ })).toBeVisible();
  await expect(stage.getByRole("button", { name: "Undo" })).toBeVisible();
  expect(await blocking(page)).toEqual([]);
});

test("Esc closes the focused window; last close returns Home", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await apply(page, "spoken", "voice");
  await expect(page.getByRole("region", { name: /TradingView window/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("region", { name: /TradingView window/ })).toHaveCount(0, { timeout: 4_000 });
  await expect(page.getByRole("navigation", { name: "Amara OS navigation" })).toBeVisible();
  await expect(page.locator(".landing__welcome")).toBeVisible();
});

test("typed pull-up opens a sample window; Close on the X works", async ({ page, theme, consoleErrors }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  const box = page.getByLabel("Message Amara");
  await box.fill("pull up bitcoin on TradingView");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByTestId("home-windows-stage")).toBeVisible({ timeout: 8_000 });
  await expect(page.getByRole("region", { name: /TradingView window/ })).toBeVisible({ timeout: 8_000 });
  await page.getByRole("button", { name: "Close TradingView (Esc)" }).click();
  await expect(page.getByRole("region", { name: /TradingView window/ })).toHaveCount(0, { timeout: 4_000 });
  expect(consoleErrors).toEqual([]);
});

test("Settings › Services lists signed-in sites with Sign out and no secrets", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await navButton(page, "Settings").click();
  await routeSettled(page).catch(() => undefined);
  const field = activeField(page, "Settings");
  const tab = field.getByRole("radio", { name: "Services", exact: true });
  if ((await tab.getAttribute("aria-checked")) !== "true") await tab.click();
  await expect(field.getByRole("heading", { name: "Sites you're signed in to inside Amara" })).toBeVisible();
  await expect(field.getByText("instagram.com")).toBeVisible();
  await expect(field.getByRole("button", { name: "Sign out of Instagram" })).toBeVisible();
  const html = await field.locator(".w-sites").innerHTML();
  expect(html).not.toMatch(/cookie\s*=|token\s*=|password\s*=|sessionid\s*=|authorization\s*:|xox[bap]-|sk_live|sk_test|ghp_|-----BEGIN/i);
  expect(await blocking(page)).toEqual([]);
});

test("reduced motion: windows still open, no transform animation on the stage", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await apply(page, "entering");
  const stage = page.getByTestId("home-windows-stage");
  await expect(stage).toBeVisible();
  await expect(stage).toHaveAttribute("data-phase", "entering");
  const motion = await page.evaluate(() => document.documentElement.dataset.reduceMotion);
  expect(motion).toBe("true");
  expect(await blocking(page)).toEqual([]);
});

test("sign-in page: Amara does not comment, and the banner says she reads nothing", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await apply(page, "instagram-sign-in");
  const stage = page.getByTestId("home-windows-stage");
  await expect(stage.getByText(/Amara never reads it/)).toBeVisible();
  await expect(stage.getByText("Amara:")).toHaveCount(0);
  await expect(stage.getByRole("button", { name: "Replay" })).toHaveCount(0);
  expect(await blocking(page)).toEqual([]);
});
