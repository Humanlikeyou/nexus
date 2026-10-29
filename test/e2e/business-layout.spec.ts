import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";

// screen-specs › Business › Content pipeline: 5 stage columns; below 900px of content width, one list grouped by stage.
async function stageBoxes(page: import("@playwright/test").Page) {
  await navButton(page, "Business").click();
  const field = activeField(page, "Business");
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Business");
  const groups = field.locator(".biz__stages .biz__stage");
  await expect(groups.first()).toBeVisible();
  return groups.evaluateAll((els) => els.filter((e) => getComputedStyle(e).display !== "none").map((e) => { const b = e.getBoundingClientRect(); return { x: Math.round(b.x), y: Math.round(b.y), text: e.querySelector("h3")?.textContent ?? "" }; }));
}

test("(l) wide: pipeline shows all 5 stages side by side", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const boxes = await stageBoxes(page);
  expect(boxes).toHaveLength(5);
  expect(new Set(boxes.map((b) => b.y)).size).toBe(1);
});

test("(l) narrow (<900px content): pipeline is one list grouped by stage, empty stages hidden", async ({ page, theme }) => {
  await page.setViewportSize({ width: 1000, height: 900 });
  await openApp(page, { theme, reduceMotion: true });
  const boxes = await stageBoxes(page);
  expect(boxes.map((b) => b.text)).toEqual(["Ideation · 1", "Drafting · 1"]);
  expect(new Set(boxes.map((b) => b.x)).size).toBe(1);
  expect(boxes[1].y).toBeGreaterThan(boxes[0].y);
});
