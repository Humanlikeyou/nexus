import { activeField, expect, navButton, openApp, openView, test } from "./fixtures/tauri-mock";

// design-1111v: Business is Enterprise. Content is its own tab, one pipeline Idea → Published;
// Overview keeps its first glance short, with one-line Tasks and Content cards linking to their tabs.
test("(l) Content: one pipeline from Idea to Published with per-stage counts", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const field = await openView(page, "Content");
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Content");
  // design-1118a: under 3 items Content is a list with one stage rail (the board comes back at 3+).
  await expect(field.getByTestId("content-list")).toBeVisible();
  const stages = field.getByRole("list", { name: "Pipeline stages" }).getByRole("listitem");
  await expect(stages).toHaveText([/Idea · 1/, /Drafting · 1/, /Review · 0/, /Scheduled · 0/, /Published · 0/]);
  await expect(field.getByText("Nothing scheduled. You schedule posts in the platform itself.")).toBeVisible();
});

test("(l) first glance: Overview's Tasks card is one line with at most two items; the Tasks tab has them all", async ({ page, theme }) => {
  const { responses } = await import("./fixtures/snapshot");
  const read_tasks = Array.from({ length: 24 }, (_, i) => ({ title: `Task ${i + 1}`, status: i % 2 ? "active" : "waiting", priority: "normal", tags: [] }));
  await openApp(page, { theme, reduceMotion: true, data: { ...responses(), read_tasks } });
  await navButton(page, "Enterprise").click();
  const field = activeField(page, "Enterprise");
  const card = field.getByRole("region", { name: "Tasks · 12 in progress, 12 waiting" });
  await expect(card).toBeVisible();
  await expect(card).toContainText("Task 1 is waiting · Task 2 is active");
  await expect(card).not.toContainText("Task 3");
  await card.getByRole("button", { name: "Open Tasks ›" }).click();
  await expect(field.getByRole("radio", { name: "Tasks", exact: true })).toHaveAttribute("aria-checked", "true");
  await expect(field.locator(".page-hero__statement")).toHaveText("12 in progress, 12 waiting");
  await expect(field.getByRole("region", { name: "In progress · 12" })).toBeVisible();
  await expect(field.getByText("Task 24")).toBeVisible();
});
