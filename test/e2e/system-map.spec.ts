import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, openApp, openSettingsTab, test } from "./fixtures/tauri-mock";

/** design-1112m: Settings › Services › System map (decision 0011). */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const sampleOn = (page: Page) => page.addInitScript(() => { localStorage.setItem("nexus.sampleAdapters", "1"); localStorage.setItem("nexus.standalonePreview", "{}"); });

test("(1112m) unwired: the map says Needs wiring, shows no dots and no sample tag, axe clean", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const field = await openSettingsTab(page, "Services");
  const map = field.locator("#system-map");
  await expect(map.getByRole("heading", { name: "System map" })).toBeVisible();
  await expect(map.getByText(/Needs wiring/).first()).toBeVisible();
  await expect(map.getByRole("button")).toHaveCount(0);
  await expect(map.getByText("Sample data")).toHaveCount(0);
  await page.waitForTimeout(300);
  expect(await blocking(page)).toEqual([]);
});

test("(1112m) sample: summary, 16 dots with state in their names, no horizontal scroll, axe clean", async ({ page, theme }) => {
  await sampleOn(page);
  await openApp(page, { theme, reduceMotion: true });
  const field = await openSettingsTab(page, "Services");
  const map = field.locator("#system-map");
  await expect(map.getByText("Sample data")).toBeVisible();
  await expect(map.getByRole("status")).toHaveText("1 broken: Crews");
  await expect(map.getByText(/^Checked /)).toBeVisible();
  await expect(map.locator("button[data-path]")).toHaveCount(16);
  await expect(map.getByRole("button", { name: "Crew runs and approvals: Broken" })).toBeVisible();
  // reduced motion: a static ring, no animation
  const anim = await map.locator('button[data-status="broken"] .sysmap__pip').evaluate((el) => getComputedStyle(el).animationName);
  expect(anim).toBe("none");
  await page.waitForTimeout(300);
  expect(await blocking(page)).toEqual([]);
  for (const width of [1100, 820]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForTimeout(150);
    expect(await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth])).toEqual([width, width]);
    await expect(map.locator("button[data-path]").first()).toBeVisible();
  }
});

test("(1112m) keyboard: Enter opens a dot's panel, Escape closes it and focus returns to the dot", async ({ page, theme }) => {
  await sampleOn(page);
  await openApp(page, { theme, reduceMotion: true });
  const field = await openSettingsTab(page, "Services");
  const map = field.locator("#system-map");
  const dot = map.getByRole("button", { name: "Crew runs and approvals: Broken" });
  await dot.focus();
  await page.keyboard.press("Enter");
  const panel = map.getByRole("group", { name: "Crew runs and approvals, details" });
  await expect(panel).toBeVisible();
  await expect(panel).toContainText("The crew service isn't answering");
  await expect(panel).toContainText("Open Docker and start the stack");
  await expect(panel).toContainText("Not seen working yet");
  expect(await blocking(page)).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(dot).toBeFocused();
});

test("(1112m) with motion allowed the broken dot blinks", async ({ page, theme }) => {
  await sampleOn(page);
  await openApp(page, { theme, reduceMotion: false });
  const field = await openSettingsTab(page, "Services");
  const pip = field.locator('#system-map button[data-status="broken"] .sysmap__pip');
  await expect(pip).toBeVisible();
  expect(await pip.evaluate((el) => getComputedStyle(el).animationName)).toBe("sysmap-blink");
});
