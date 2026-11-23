import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, openApp, openView, routeSettled, test } from "./fixtures/tauri-mock";

/** design-1111r option B "Horizon": Recovery and Effort in the Fitness top row, next to Sleep card B. */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const row = (p: Page) => p.locator(".fitness__row--today").first();

for (const sample of [false, true]) {
  test(`(1111r) Recovery and Effort${sample ? " with sample adapters" : ""}: Horizon layout, honest tags, axe clean`, async ({ page, theme }) => {
    if (sample) await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
    await openApp(page, { theme, reduceMotion: true });
    await openView(page, "Fitness");
    await routeSettled(page).catch(() => undefined);
    const recovery = row(page).getByRole("region", { name: "Recovery" });
    const effort = row(page).getByRole("region", { name: "Effort" });
    await expect(recovery.getByRole("meter", { name: "Recovery" })).toHaveAttribute("aria-valuenow", "74");
    await expect(recovery.getByRole("list", { name: "What moved recovery" }).getByRole("listitem")).toHaveCount(2);
    await expect(recovery).not.toContainText(/sleep/i);
    await expect(recovery.getByRole("img", { name: /^Recovery, last 7 days/ })).toBeVisible();
    await expect(effort.getByRole("meter", { name: "Effort" })).toBeVisible();
    await expect(effort).toContainText("Zone minutes");
    if (sample) {
      await expect(effort.getByText("Sample data")).toBeVisible();
      await expect(recovery.getByText("Sample data")).toHaveCount(0);
    } else {
      await expect(effort.getByText("Needs wiring")).toBeVisible();
      await expect(page.getByText("Sample data")).toHaveCount(0);
    }
    // Recovery and Effort sit in their columns at the same top; nothing spills out of a card.
    const boxes = await row(page).locator(":scope > *").evaluateAll((els) => els.map((e) => { const b = e.getBoundingClientRect(); return { top: b.top, right: b.right, inner: [...e.querySelectorAll("*")].every((c) => c.getBoundingClientRect().right <= b.right + .5) }; }));
    expect(boxes).toHaveLength(3);
    expect(boxes.every((b) => b.inner)).toBe(true);
    expect(Math.abs(boxes[0].top - boxes[1].top)).toBeLessThanOrEqual(1);
    await page.waitForTimeout(300);
    expect(await blocking(page)).toEqual([]);
  });
}
