import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, navButton, openApp, openSettingsTab, test } from "./fixtures/tauri-mock";

const banned = /navy mode|red light|quiet mode/i;
const sample = (p: Page) => p.addInitScript(() => { localStorage.setItem("nexus.sampleAdapters", "1"); });

test("Amara, go dark turns the look on, with no name on screen", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  const chat = page.locator(".landing .hchat");
  await expect(chat).toBeVisible();
  const box = chat.getByRole("textbox").first();
  await box.fill("Amara, go dark.");
  await box.press("Enter");
  await expect.poll(() => page.evaluate(() => document.documentElement.getAttribute("data-navy"))).toBe("on");
  const status = page.getByRole("status").filter({ hasText: /Lights out|Going dark|with you|leaves this Mac|just us|got smaller|All quiet|off the grid/ });
  await expect(status).toBeVisible();
  await expect(status).toContainText("Sample data");
  await expect(page.locator("body")).not.toContainText(banned);
});

test("back to normal turns it off, and a reload without a preview key stays off", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  const box = page.locator(".landing .hchat").getByRole("textbox").first();
  await box.fill("Amara, go dark.");
  await box.press("Enter");
  await expect.poll(() => page.evaluate(() => document.documentElement.getAttribute("data-navy"))).toBe("on");
  await box.fill("Amara, back to normal.");
  await box.press("Enter");
  await expect.poll(() => page.evaluate(() => document.documentElement.getAttribute("data-navy"))).toBeNull();
  await page.reload();
  await expect(page.locator(".landing")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.getAttribute("data-navy"))).toBeNull();
});

test("Settings has no toggle for this look", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  await openSettingsTab(page, "General");
  const field = page.getByRole("region", { name: "Settings system" });
  await expect(field.getByText("Theme")).toBeVisible();
  await expect(field).not.toContainText(/go dark|navy|red light/i);
});

test("sample preview trigger can freeze it on, and holding the orb asks to go back", async ({ page, theme }) => {
  await page.addInitScript(() => {
    localStorage.setItem("nexus.sampleAdapters", "1");
    localStorage.setItem("nexus.navyPreview", JSON.stringify({ scenario: "confirm", openedBy: "voice" }));
  });
  await openApp(page, { theme, reduceMotion: true });
  await expect.poll(() => page.evaluate(() => document.documentElement.getAttribute("data-navy"))).toBe("on");
  await expect(page.getByRole("dialog", { name: "Back to normal?" })).toBeVisible();
  await page.getByRole("button", { name: "Confirm" }).click();
  await expect.poll(() => page.evaluate(() => document.documentElement.getAttribute("data-navy"))).toBeNull();
});

async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}

test("settled look is axe clean and the accent is steel, not emerald", async ({ page, theme }) => {
  await page.addInitScript(() => {
    localStorage.setItem("nexus.sampleAdapters", "1");
    localStorage.setItem("nexus.navyPreview", JSON.stringify({ scenario: "on", openedBy: "voice" }));
  });
  await openApp(page, { theme, reduceMotion: true });
  await expect.poll(() => page.evaluate(() => document.documentElement.getAttribute("data-navy"))).toBe("on");
  const tokens = await page.evaluate(() => {
    const s = getComputedStyle(document.documentElement);
    const pick = (n) => s.getPropertyValue(n).trim().toLowerCase();
    const resolve = (n) => {
      const el = document.createElement("span");
      el.style.color = `var(${n})`;
      document.documentElement.appendChild(el);
      const c = getComputedStyle(el).color;
      el.remove();
      return c;
    };
    return {
      steel: pick("--nv-steel"),
      fill: pick("--nv-steel-fill"),
      danger: pick("--nv-amber"),
      steelRgb: resolve("--nv-steel"),
      focusRgb: resolve("--focus-ring"),
      dangerRgb: resolve("--tone-danger"),
    };
  });
  expect(tokens.steel).toBe("#8fb8d0");
  expect(tokens.fill).toBe("#35617c");
  expect(tokens.danger).toBe("#f0b429");
  expect(tokens.steelRgb).toBe("rgb(143, 184, 208)");
  expect(tokens.focusRgb).toBe("rgb(143, 184, 208)");
  expect(tokens.dangerRgb).toBe("rgb(240, 180, 41)");
  expect(await blocking(page)).toEqual([]);
});

test("Today (toolbar visible) is axe clean in both themes", async ({ page, theme }) => {
  await page.addInitScript(() => {
    localStorage.setItem("nexus.sampleAdapters", "1");
    localStorage.setItem("nexus.navyPreview", JSON.stringify({ scenario: "on", openedBy: "voice" }));
  });
  await openApp(page, { theme, reduceMotion: true });
  await navButton(page, "Today").click();
  await expect(page.locator(".toolbar-search")).toBeVisible();
  expect(await blocking(page)).toEqual([]);
});
