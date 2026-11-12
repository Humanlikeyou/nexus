import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, gotoToday, navButton, openApp, test } from "./fixtures/tauri-mock";

// design-1112g: Life › Goals habit tracker + goal roadmaps, sample and unwired, both themes.
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
async function openGoals(page: Page) {
  await gotoToday(page);
  await navButton(page, "Life").click();
  const field = activeField(page, "Life");
  await field.getByRole("radio", { name: "Goals", exact: true }).click();
  await expect(field.getByRole("heading", { level: 1, name: "Goals" })).toBeVisible();
  return field;
}

test("Life › Goals with sample adapters: today strip, 12-week heatmaps, roadmap, axe clean", async ({ page, theme, consoleErrors }) => {
  await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true });
  const field = await openGoals(page);
  await expect(field.getByRole("list", { name: "Habits today" })).toBeVisible();
  expect(await field.locator(".gl-heat").count()).toBeGreaterThan(0);
  await expect(field.locator(".gl-heat").first().locator("i")).toHaveCount(84);
  await expect(field.getByText("Sample data").first()).toBeVisible();
  const rm = field.locator(".gl-rm").first();
  await expect(rm).toHaveAttribute("open", "");
  await expect(rm.getByRole("button", { name: "Accept" })).toBeVisible();
  await expect(rm.getByRole("button", { name: /^Yes, I did/ }).first()).toBeVisible();
  await expect(rm.getByText(/A smaller first move/)).toBeVisible();
  await rm.getByRole("button", { name: "Accept" }).click();
  await expect(rm.getByRole("button", { name: "Accept" })).toHaveCount(0);
  expect(await blocking(page)).toEqual([]);
  expect(consoleErrors).toEqual([]);
});

test("Life › Goals unwired: honest notes, no fake ticks, axe clean", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true });
  const field = await openGoals(page);
  await expect(field.getByText("Sample data")).toHaveCount(0);
  await expect(field.locator(".gl-heat i[data-m='met']")).toHaveCount(0);
  await expect(field.getByTestId("roadmap-unwired").first()).toBeVisible();
  await expect(field.getByRole("button", { name: "Draft a roadmap tonight" })).toHaveCount(0);
  expect(await blocking(page)).toEqual([]);
  expect(consoleErrors).toEqual([]);
});

// design-1112h: today's one step question lives inside Amara's take (first take he sees), once per day.
test("Step question in the take (sample): shown once, Yes confirms with the roadmap's percentage, then gone; axe clean", async ({ page, theme, consoleErrors }) => {
  await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true });
  await gotoToday(page);
  await navButton(page, "Life").click();
  const field = activeField(page, "Life");
  const q = field.getByRole("region", { name: "Step question" });
  await expect(q).toHaveCount(1);
  await expect(q).toContainText("Have you done:");
  await expect(q.getByText("Sample data")).toBeVisible();
  expect(await blocking(page)).toEqual([]);
  await q.getByRole("button", { name: "Yes", exact: true }).click();
  await expect(q.getByRole("status")).toContainText(/is now \d+% of the roadmap done\./);
  expect(await blocking(page)).toEqual([]);
  // Another take later the same day does not ask again.
  await navButton(page, "Enterprise").click();
  await expect(page.getByRole("region", { name: "Step question" })).toHaveCount(0);
  expect(consoleErrors).toEqual([]);
});
