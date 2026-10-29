import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";

const SECTIONS = ["Home", "Chat & Agents", "Fitness", "Neuro", "Business", "Settings"];

async function blockingViolations(page: Page, disableRules: string[] = [], exclude: string[] = []) {
  let builder = new AxeBuilder({ page }).disableRules(disableRules);
  for (const selector of exclude) builder = builder.exclude(selector);
  const results = await builder.analyze();
  return results.violations
    .filter((v) => v.impact === "serious" || v.impact === "critical")
    .map((v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 5).map((n) => n.target.join(" ")).join(" | ")}`);
}

async function openSection(page: Page, theme: "light" | "dark", label: string) {
  await openApp(page, { theme, reduceMotion: true });
  await navButton(page, label).click();
  await expect(activeField(page, label).getByRole("heading", { level: 1 })).toBeVisible();
  // Let the 200 ms content transitions settle so colors are final.
  await page.waitForTimeout(400);
}

for (const label of SECTIONS) {
  test(`(g) ${label}: no serious or critical axe violations (except known color-contrast)`, async ({ page, theme }) => {
    await openSection(page, theme, label);
    expect(await blockingViolations(page, ["color-contrast"])).toEqual([]);
  });

  // Label tiers are solid hex (Decision 18), the wordmark is the drawn SVG (Decision 17) and Badge text uses
  // solid tone tokens on a 14% tint (Decision 20), so nothing is excluded from color-contrast any more.
  test(`(g) ${label}: no serious or critical axe violations at all`, async ({ page, theme }) => {
    await openSection(page, theme, label);
    expect(await blockingViolations(page)).toEqual([]);
  });
}
