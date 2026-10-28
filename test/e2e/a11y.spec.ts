import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";

const SECTIONS = ["Home", "Chat & Agents", "Fitness", "Neuro", "Business", "Settings"];

async function blockingViolations(page: Page, disableRules: string[] = []) {
  const results = await new AxeBuilder({ page }).disableRules(disableRules).analyze();
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

  // BUG: color-contrast (serious) in every section, both themes. Secondary and
  // tertiary label tokens are below WCAG AA 4.5:1 for 10–13 px text:
  // src-console/styles/tokens.css:24 --label-secondary rgba(60,60,67,.60) → #86868b
  // on #f5f5f7 = 3.32:1; :25 --label-tertiary rgba(60,60,67,.30) → #bebec1 = 1.7:1
  // (sidebar group h2, .wordmark-os, toolbar clock, sidebar status, fitness meta/legends);
  // dark mode maps the same tokens at :182-183 and :221-222. Also .status-indicator
  // (#34c759 on #f5f5f7 = 2.03:1) and success Badge (3.73:1). Needs a token decision.
  test.fixme(`(g) ${label}: no serious or critical axe violations at all`, async ({ page, theme }) => {
    await openSection(page, theme, label);
    expect(await blockingViolations(page)).toEqual([]);
  });
}
