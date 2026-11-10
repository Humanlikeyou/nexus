import { activeField, expect, navButton, openApp, openMore, test } from "./fixtures/tauri-mock";

// Business keeps its first glance short: the pipeline lives behind the disclosure, as one list grouped by stage.
test("(l) pipeline: one list grouped by stage, empty stages hidden", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  await navButton(page, "Business").click();
  const field = activeField(page, "Business");
  // design-1110d D-B1: the work list is the Work tab; Overview shows the top of it in a card.
  await field.getByRole("radio", { name: "Work", exact: true }).click();
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Business");
  await expect(field.locator(".biz__stage").first()).toBeHidden();
  await openMore(page);
  await expect(field.locator(".biz__stage").first()).toBeVisible();
  const boxes = await field.locator(".biz__stage").evaluateAll((els) => els.map((e) => { const b = e.getBoundingClientRect(); return { x: b.x, y: b.y, text: e.querySelector(".ui-inset-group__title")?.textContent }; }));
  expect(boxes.map((b) => b.text)).toEqual(["Ideation · 1", "Drafting · 1"]);
  expect(new Set(boxes.map((b) => b.x)).size).toBe(1);
  expect(boxes[1].y).toBeGreaterThan(boxes[0].y);
});

test("(l) first glance: a headline and at most three work items, however long the list", async ({ page, theme }) => {
  const { responses } = await import("./fixtures/snapshot");
  const read_tasks = Array.from({ length: 24 }, (_, i) => ({ title: `Task ${i + 1}`, status: i % 2 ? "active" : "waiting", priority: "normal", tags: [] }));
  await openApp(page, { theme, reduceMotion: true, data: { ...responses(), read_tasks } });
  await navButton(page, "Business").click();
  const field = activeField(page, "Business");
  // design-1110d D-B1: the work list is the Work tab; Overview shows the top of it in a card.
  await field.getByRole("radio", { name: "Work", exact: true }).click();
  await expect(field.locator(".page-hero__statement")).toHaveText("12 in progress, 12 waiting");
  await expect(field.locator(".quiet-line")).toHaveCount(3);
  await expect(field.getByText("Task 24")).toBeHidden();
});
