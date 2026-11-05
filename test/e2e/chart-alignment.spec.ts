import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, openMore, openView, routeSettled, test } from "./fixtures/tauri-mock";

/** Every plot on Mind–Body shares one column: these edges are measured, so it can't drift by eye. */
const SIZES = [{ width: 1280, height: 800 }, { width: 1440, height: 900 }, { width: 900, height: 620 }];
const TOLERANCE = 1;

type Box = { left: number; right: number };
const boxes = (page: Page, selector: string) => page.locator(selector).evaluateAll((els) => els.filter((e) => e.getBoundingClientRect().width > 0).map((e) => { const r = e.getBoundingClientRect(); return { left: r.left, right: r.right, name: e.className.toString() || e.tagName }; }));
const sameEdges = (items: Box[], expected: Box, label: string) => {
  for (const item of items) {
    expect(Math.abs(item.left - expected.left), `${label}: left edge`).toBeLessThanOrEqual(TOLERANCE);
    expect(Math.abs(item.right - expected.right), `${label}: right edge`).toBeLessThanOrEqual(TOLERANCE);
  }
};

async function openOverview(page: Page, theme: "light" | "dark") {
  await openApp(page, { theme });
  await navButton(page, "Mind–Body").click();
  const field = activeField(page, "Mind–Body");
  await field.locator(".mb-chart").first().waitFor();
  await routeSettled(page);
  return field;
}

for (const size of SIZES) {
  test.describe(`${size.width}x${size.height}`, () => {
    test.beforeEach(async ({ page }) => { await page.setViewportSize(size); });

    test("Mind–Body: every plot shares the same left and right edges", async ({ page, theme }) => {
      await openOverview(page, theme);
      for (const range of ["Month", "Week"]) {
        await page.getByRole("radio", { name: range, exact: true }).click();
        await page.waitForTimeout(50);
        const plots = await boxes(page, ".mind-body [data-plot]");
        expect(plots.length).toBeGreaterThanOrEqual(8);
        sameEdges(plots, plots[0], range);
        // Stage strip, sleep chart, four trends, load bars and the scatter are all on it.
        expect((await boxes(page, ".mind-body .mind-body__chart svg")).length).toBeGreaterThanOrEqual(2);
        sameEdges(await boxes(page, ".mind-body .mind-body__chart svg"), plots[0], `${range} svg`);
      }
    });

    test("Mind–Body: labels, drivers and the table sit on the same grid as the plots", async ({ page, theme }) => {
      await openOverview(page, theme);
      const [plot] = await boxes(page, ".mind-body [data-plot]");
      const [section] = await boxes(page, ".mb-section");
      for (const head of await boxes(page, ".mb-row__head")) expect(Math.abs(head.left - section.left)).toBeLessThanOrEqual(TOLERANCE);
      for (const value of await boxes(page, ".mb-drivers__value")) expect(Math.abs(value.left - plot.left), "driver values").toBeLessThanOrEqual(TOLERANCE);
      const [week] = await boxes(page, ".mb-table thead th:nth-child(2)");
      expect(Math.abs(week.left - plot.left), "table columns").toBeLessThanOrEqual(TOLERANCE);
      for (const line of await boxes(page, ".mb-drivers__usual")) expect(Math.abs(line.right - section.right), "usual column").toBeLessThanOrEqual(TOLERANCE);
    });

    test("Mind–Body: nothing overflows its plot", async ({ page, theme }) => {
      await openOverview(page, theme);
      const inside = await page.locator(".mb-chart").evaluateAll((charts) => charts.map((chart) => {
        const box = chart.getBoundingClientRect();
        const dot = chart.querySelector(".mb-chart__now")?.getBoundingClientRect();
        const line = chart.querySelector("path")?.getBoundingClientRect();
        return { dot: dot ? dot.left >= box.left - .5 && dot.right <= box.right + .5 : true, line: line ? line.left >= box.left - .5 && line.right <= box.right + .5 : true };
      }));
      expect(inside.length).toBeGreaterThanOrEqual(5);
      expect(inside.every((item) => item.dot && item.line)).toBe(true);
      const bars = await boxes(page, ".mind-body__bar");
      const chart = (await boxes(page, ".mind-body__chart"))[0];
      expect(Math.abs(bars[0].left - chart.left), "first bar").toBeLessThanOrEqual(TOLERANCE);
      expect(Math.abs(bars[bars.length - 1].right - chart.right), "last bar").toBeLessThanOrEqual(TOLERANCE);
    });

    test("Fitness: rings and detail rows share three columns", async ({ page, theme }) => {
      await openApp(page, { theme });
      await openView(page, "Fitness");
      await openMore(page);
      const cards = await boxes(page, ".fitness__row--today > *");
      expect(cards).toHaveLength(3);
      const rings = await boxes(page, ".fitness__ring-body .ui-ring");
      for (const [i, ring] of rings.entries()) expect(Math.abs(ring.left - cards[i].left), "ring on its column").toBeLessThanOrEqual(TOLERANCE);
      const tiles = await boxes(page, ".fitness__tiles > *");
      for (const tile of tiles) expect(Math.min(...cards.map((card) => Math.abs(card.left - tile.left))), "tile on a column").toBeLessThanOrEqual(TOLERANCE);
    });
  });
}
