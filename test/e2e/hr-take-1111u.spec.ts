import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, gotoToday, openApp, routeSettled, test } from "./fixtures/tauri-mock";

/**
 * build-1111u, changed 2026-11-12: Taisun took the heart-rate take (sentence, Avg / Range / Trend) off Home and off
 * Today's "Your body today" card. Backend and the hrTake components stay; this spec now asserts they are NOT rendered,
 * while the live 89-second trace and readiness are kept.
 */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
for (const sample of [false, true]) {
  test(`(1111u) Home and Today${sample ? " with sample adapters" : ""}: no heart-rate take, live trace kept, axe clean`, async ({ page, theme, consoleErrors }) => {
    if (sample) await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
    await openApp(page, { theme, reduceMotion: true });
    await expect(page.locator(".landing__heart-rate")).toHaveAttribute("aria-label", /^Heart rate \d+ beats per minute$/);
    await expect(page.locator(".landing__welcome").getByText(/you were at \d+/)).toHaveCount(0);
    await expect(page.locator(".hrt")).toHaveCount(0);
    expect(await blocking(page)).toEqual([]);
    const field = await gotoToday(page);
    await routeSettled(page).catch(() => undefined);
    const card = field.getByRole("region", { name: "Your body today" });
    await expect(card).toBeVisible();
    await expect(card.getByRole("meter", { name: "Recovery" })).toBeVisible();
    await expect(card.locator(".hrt")).toHaveCount(0);
    await expect(card.getByText(/^(Avg|Range|Trend)$/)).toHaveCount(0);
    expect(await blocking(page)).toEqual([]);
    expect(consoleErrors).toEqual([]);
  });
}
