import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, routeSettled, test } from "./fixtures/tauri-mock";

/** design-1111w added scope: Life pages show Amara's take (same AmaraTake card as other sections). */
const sample = (page: Page) => page.addInitScript(() => { localStorage.setItem("nexus.sampleAdapters", "1"); });
const settle = async (p: Page) => { await routeSettled(p).catch(() => undefined); await p.waitForTimeout(300); };
const lifeTab = async (p: Page, label: string) => {
  await navButton(p, "Life").click(); await settle(p);
  const r = activeField(p, "Life").getByRole("radio", { name: label, exact: true });
  if ((await r.getAttribute("aria-checked")) !== "true") await r.click();
  await settle(p);
  return activeField(p, "Life");
};
const lifeTake = (host: ReturnType<typeof activeField>) => host.locator(".life__take").getByRole("region", { name: /^Amara's take$/ });

test("Life Goals, Weekly review, Dates and Personal money each show Amara's take", async ({ page, theme, consoleErrors }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  for (const label of ["Goals", "Weekly review", "Dates & deadlines", "Personal money"] as const) {
    const field = await lifeTab(page, label);
    await expect(field.getByRole("heading", { level: 1, name: label === "Dates & deadlines" ? "Dates & deadlines" : label === "Personal money" ? "Personal money" : label })).toBeVisible();
    const take = lifeTake(field);
    await expect(take).toBeVisible();
    await expect(take).toHaveAttribute("data-state", "fresh");
    await expect(take).toContainText("Sample data");
    await expect(take).toContainText("4 of 5");
  }
  expect(consoleErrors).toEqual([]);
});

test("without sample adapters, the Life take is Needs wiring and invents no numbers", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const field = await lifeTab(page, "Goals");
  const take = lifeTake(field);
  await expect(take).toBeVisible();
  await expect(take).toHaveAttribute("data-state", "unwired");
  await expect(take).toContainText("Needs wiring");
  await expect(take).not.toContainText("4 of 5");
});
