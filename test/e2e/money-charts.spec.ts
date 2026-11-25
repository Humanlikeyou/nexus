import AxeBuilder from "@axe-core/playwright";
import { FULL, TODAY, VIEW } from "../../src-console/money/fixtures";
import { activeField, expect, navButton, openApp, routeSettled, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** Personal money page: charts, Amara's money read and the manual FX form render from the month view, axe clean. MONEY_SHOTS=<dir> saves screenshots. */
test("Personal money charts, money read and FX form: render, hover, axe clean, no yen sign", async ({ page, theme, consoleErrors }) => {
  const data = { ...responses(), money_month_view: { ...VIEW, today: TODAY, fx: { rate: 151.2, date: "2026-11-10", source: "manual" } }, money_entries_list: { entries: [], skipped_lines: 0 }, fx_rate_set_manual: { rate: 152, date: "2026-11-11", source: "manual", fetched_at: "2026-11-11T09:00:00+09:00" } };
  await openApp(page, { theme, reduceMotion: true, data });
  await navButton(page, "Life").click();
  await activeField(page, "Life").getByRole("radio", { name: "Personal money", exact: true }).click();
  await routeSettled(page).catch(() => undefined);
  const field = activeField(page, "Life");
  await expect(field.getByRole("heading", { name: "Income and spending by month" })).toBeVisible();
  await expect(field.getByRole("heading", { name: "Spending by category, 2026-11" })).toBeVisible();
  await expect(field.getByText("Amara's money read")).toBeVisible();
  await expect(field.getByText(/Stored rate: 151.2 JPY per 1 USD, as of 2026-11-10/)).toBeVisible();
  await expect(field.locator(".trend-chart").first()).toBeVisible();
  const box = await field.locator(".trend-chart svg").first().boundingBox();
  if (box) { await page.mouse.move(box.x + box.width * 0.6, box.y + box.height / 2); await expect(field.locator(".trend-chart__tip").first()).toBeVisible(); }
  await field.getByText("Assumptions").click();
  expect(FULL.currency).toBe("JPY");
  if (process.env.MONEY_SHOTS) {
    const dir = process.env.MONEY_SHOTS;
    await field.locator(".mc-grid").first().screenshot({ path: `${dir}/charts-${theme}.png` });
    await field.locator(".ui-card").filter({ hasText: "Amara's money read" }).last().screenshot({ path: `${dir}/advice-${theme}.png` });
  }
  const r = await new AxeBuilder({ page }).analyze();
  expect(r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`)).toEqual([]);
  expect(await field.textContent()).not.toMatch(/[¥￥]/);
  expect(consoleErrors).toEqual([]);
});
