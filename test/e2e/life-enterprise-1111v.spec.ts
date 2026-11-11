import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, gotoToday, navButton, openApp, openView, routeSettled, test } from "./fixtures/tauri-mock";

// design-1111v (Taisun 19:23 JST): Today is a lean day, Life holds personal pages, Business is renamed Enterprise.
const sample = (page: Page) => page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
const tab = (page: Page, host: string, label: string) => activeField(page, host).getByRole("radio", { name: label, exact: true });

async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}

test("sidebar: Life ⌘4 and Enterprise ⌘5 after Mind–Body; no Business; Settings ⌘6, Smart home ⌘7", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true });
  await gotoToday(page);
  const nav = page.getByRole("navigation", { name: "Amara OS navigation" });
  await expect(nav.getByRole("button", { name: /^Business/ })).toHaveCount(0);
  await expect(nav.locator(".nav-shortcut")).toHaveText(["⌘1", "⌘2", "⌘3", "⌘4", "⌘5", "⌘6", "⌘7"]);
  await expect(nav.getByRole("button", { name: /^Life/ })).toContainText("⌘4");
  await expect(nav.getByRole("button", { name: /^Enterprise/ })).toContainText("⌘5");
  await navButton(page, "Life").click();
  await expect(activeField(page, "Life")).toBeVisible();
  await expect(tab(page, "Life", "Goals")).toHaveAttribute("aria-checked", "true");
  for (const label of ["Goals", "Weekly review", "Dates & deadlines", "Personal money"]) await expect(tab(page, "Life", label)).toBeVisible();
  await navButton(page, "Enterprise").click();
  await expect(activeField(page, "Enterprise")).toBeVisible();
  await expect(activeField(page, "Enterprise").getByRole("heading", { level: 1, name: "Enterprise" })).toBeVisible();
  for (const label of ["Overview", "Decisions", "Ventures", "Finances", "Tasks", "Content"]) await expect(tab(page, "Enterprise", label)).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test("old routes keep working: palette names, the stored session view and in-page links", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  // A Business › Money / Revenue view stored by an older build restores as Enterprise › Finances.
  await page.evaluate(() => sessionStorage.setItem("amara.business.view", "money"));
  await navButton(page, "Enterprise").click();
  await expect(tab(page, "Enterprise", "Finances")).toHaveAttribute("aria-checked", "true");
  await expect(activeField(page, "Enterprise").getByRole("heading", { level: 1, name: "Finances" })).toBeVisible();
  // Old names still find the page in the palette.
  for (const [query, host, label] of [["revenue", "Enterprise", "Finances"], ["life admin", "Life", "Dates & deadlines"], ["trajectory", "Life", "Goals"]] as const) {
    await page.keyboard.press("Meta+k");
    await page.getByRole("combobox", { name: "Search or ask Amara" }).fill(query);
    await page.keyboard.press("Enter");
    await expect(tab(page, host, label)).toHaveAttribute("aria-checked", "true");
  }
  // Finances links to Life › Personal money (personal money moved out of Enterprise).
  await openView(page, "Finances");
  await activeField(page, "Enterprise").getByRole("button", { name: /Life › Personal money/ }).click();
  await expect(tab(page, "Life", "Personal money")).toHaveAttribute("aria-checked", "true");
  await expect(activeField(page, "Life")).not.toContainText("Kept apart");
});

test("Today: card C first, Needs you today second, then check-in and To-dos; no Goals, Life admin, Weekly review", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true });
  const today = await gotoToday(page);
  await expect(today.getByRole("heading", { level: 1, name: "Today" })).toBeVisible();
  const ids = await today.locator(".home").evaluate((home) => [...home.children].map((c) => c.id || [...c.classList].find((k) => k.startsWith("home__")) || c.tagName.toLowerCase()));
  const order = ids.filter((x) => ["home__recovery-card", "needs-you", "checkin", "lists"].includes(x));
  expect(order).toEqual(["home__recovery-card", "needs-you", "checkin", "lists"]);
  for (const id of ["goals", "life-admin", "weekly-review"]) await expect(today.locator(`#${id}`)).toHaveCount(0);
  await expect(today.getByRole("region", { name: "Revenue today" })).toHaveCount(0);
  await expect(today.getByText("Your weekly review is ready ›")).toHaveCount(0);
  // The check-in is a quiet row all day and isn't counted.
  const inbox = today.getByRole("list", { name: "Needs you today" });
  await expect(inbox.locator('li[data-source="checkin"]')).toHaveAttribute("data-quiet", "true");
  await expect(inbox.locator('li[data-source="checkin"]')).toContainText("Evening check-in");
  expect(consoleErrors).toEqual([]);
});

test("Sunday: the weekly-review card sits under card C and opens Life › Weekly review", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true, now: "2026-10-25T14:00:00+09:00" });
  const today = await gotoToday(page);
  await expect(today.locator(".home__summary")).toContainText("Sunday, Oct 25");
  const ready = today.getByRole("button", { name: "Your weekly review is ready. Open Life, Weekly review" });
  await expect(ready).toBeVisible();
  expect(await ready.evaluate((el) => el.previousElementSibling?.classList.contains("home__recovery-card"))).toBe(true);
  await ready.click();
  await expect(tab(page, "Life", "Weekly review")).toHaveAttribute("aria-checked", "true");
});

test("live mode: every add is disabled with its reason; nothing sample shows", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const life = await openView(page, "Dates & deadlines");
  await expect(life.getByRole("button", { name: "Add", exact: true })).toBeDisabled();
  await expect(life.getByText("Needs wiring · life_date_upsert").first()).toBeVisible();
  await expect(life.getByText("Sample data")).toHaveCount(0);
  const ent = await openView(page, "Tasks");
  await expect(ent.getByRole("button", { name: "Add task" })).toBeDisabled();
  await openView(page, "Decisions");
  await expect(ent.getByRole("heading", { level: 1, name: "Decisions" })).toBeVisible();
  // Live: no decisions log store yet, so no log table and no made-up rows.
  await expect(ent.getByRole("table", { name: "Decisions log" })).toHaveCount(0);
  await expect(ent.getByText("Sample data")).toHaveCount(0);
});

test("mock mode: Dates composer asks for the year, previews, saves with Undo; Akiya is in the Personal lane", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  const life = await openView(page, "Dates & deadlines");
  const tl = life.getByRole("img", { name: /Dates over the next 12 months/ });
  await expect(tl).toHaveAttribute("aria-label", /Akiya offer deadline, Dec 18, Personal/);
  // Lane labels never overlap (mock gap: crowded labels).
  const boxes = await tl.locator("text.lbl").evaluateAll((ts) => ts.map((t) => {
    // The label and its date line (the lead line from the dot is allowed to pass under other labels).
    const a = (t as SVGGraphicsElement).getBBox(), b = (t.nextElementSibling as SVGGraphicsElement).getBBox();
    const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
    return [x, y, Math.max(a.x + a.width, b.x + b.width) - x, Math.max(a.y + a.height, b.y + b.height) - y];
  }));
  for (let i = 0; i < boxes.length; i++) for (let j = i + 1; j < boxes.length; j++) {
    const [a, b] = [boxes[i], boxes[j]];
    // 2 px of glyph-box slack: text boxes include line-gap padding.
    const overlap = a[0] + 2 < b[0] + b[2] && b[0] + 2 < a[0] + a[2] && a[1] + 2 < b[1] + b[3] && b[1] + 2 < a[1] + a[3];
    expect(overlap, `labels ${i} and ${j}`).toBe(false);
  }
  const input = life.getByRole("textbox", { name: "Type it the way you'd say it" });
  await input.fill("Passport renewal Mar 3");
  await life.getByRole("button", { name: "Add", exact: true }).click();
  await life.getByRole("region", { name: "Question: Which year is it?" }).getByRole("button", { name: "2027" }).click();
  const preview = life.getByRole("region", { name: "Preview: Date" });
  await expect(preview).toContainText("Mar 3, 2027");
  await preview.getByRole("button", { name: "Save date" }).click();
  await expect(life.getByRole("button", { name: /^Undo/ })).toBeVisible();
  await life.getByRole("button", { name: /^Undo/ }).click();
  await expect(life.getByText(/Undone\./)).toBeVisible();
  // Business dates are sent to Enterprise, not added here (one write path).
  await input.fill("Taiyo launch moved to Nov 28");
  await life.getByRole("button", { name: "Add", exact: true }).click();
  await expect(life.getByText(/Business dates live in Enterprise/)).toBeVisible();
});

test("mock mode: Record vote writes one log row, sends nothing, Undo removes it", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  const ent = await openView(page, "Decisions");
  const log = ent.getByRole("table", { name: "Decisions log" });
  const rows = await log.getByRole("row").count();
  await ent.getByRole("button", { name: "Vote yes" }).first().click();
  const preview = ent.getByRole("region", { name: "Preview: vote" });
  await expect(preview).toContainText("Nothing moves money. Nothing is sent.");
  const before = await page.evaluate(() => (window as unknown as { __E2E_CALLS: { cmd: string }[] }).__E2E_CALLS.length);
  await preview.getByRole("button", { name: "Record vote" }).click();
  await expect(log.getByRole("row")).toHaveCount(rows + 1);
  const calls = await page.evaluate((n) => (window as unknown as { __E2E_CALLS: { cmd: string }[] }).__E2E_CALLS.slice(n).map((c) => c.cmd), before);
  expect(calls.filter((c) => /ledger|fund|send|transfer|approve/i.test(c))).toEqual([]);
  await ent.getByRole("button", { name: /^Undo/ }).click();
  await expect(log.getByRole("row")).toHaveCount(rows);
});

test("Enterprise › Ventures has no Akiya; Life › Dates & deadlines has it", async ({ page, theme }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  const ent = await openView(page, "Ventures");
  await routeSettled(page).catch(() => undefined);
  await expect(ent.getByRole("heading", { level: 1, name: "Ventures" })).toBeVisible();
  await expect(ent.locator(".pk-list, .ui-list").getByText(/^Akiya/)).toHaveCount(0);
  const life = await openView(page, "Dates & deadlines");
  await expect(life.getByRole("table", { name: "Upcoming" })).toContainText("Akiya offer");
});

for (const mode of ["live", "sample"] as const) {
  for (const label of ["Goals", "Weekly review", "Dates & deadlines", "Personal money", "Overview", "Decisions", "Ventures", "Finances", "Tasks", "Content", "Today"]) {
    test(`axe (${mode}): ${label} has no serious or critical violations`, async ({ page, theme }) => {
      if (mode === "sample") await sample(page);
      await openApp(page, { theme, reduceMotion: true });
      const field = label === "Today" ? await gotoToday(page) : await openView(page, label);
      await expect(field.getByRole("heading", { level: 1 })).toBeAttached();
      await page.waitForTimeout(400);
      expect(await blocking(page)).toEqual([]);
    });
  }
}
