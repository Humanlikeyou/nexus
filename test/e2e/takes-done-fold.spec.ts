import type { Page } from "@playwright/test";
import { expect, openApp, openView, test } from "./fixtures/tauri-mock";

/** 2026-11-14 (Taisun): Done folds "Today's one thing" into one slim line for the rest of the day; Undo opens it again. Fixtures only. */
const SHOT = process.env.NEXUS_SHOT_DIR ?? "";
const preview = (page: Page, takes: Record<string, unknown> = {}) => page.addInitScript((t) => {
  localStorage.setItem("nexus.sampleAdapters", "1");
  localStorage.setItem("nexus.takesPreview", JSON.stringify(t));
}, takes);
const shot = async (page: Page, name: string, theme: string) => { if (SHOT) await page.screenshot({ path: `${SHOT}/${name}-${theme}.png` }); };
const homeBlock = (page: Page) => page.locator(".landing .take-slot .t-block");

test("(done-fold) Home: Done slides the suggestion into a slim line, Undo brings it back, no scroll, reduced motion has no transition", async ({ page, theme }) => {
  await preview(page);
  await openApp(page, { theme, reduceMotion: true });
  const block = homeBlock(page);
  await expect(block).toBeVisible();
  const group = block.getByRole("group", { name: /Your answer/ });
  await expect(group).toBeVisible();
  await shot(page, "home-expanded", theme);
  const noScroll = () => page.evaluate(() => { const m = document.querySelector(".landing") as HTMLElement | null; return m ? m.scrollHeight <= m.clientHeight + 1 : true; });
  expect(await noScroll()).toBe(true);
  await group.getByRole("button", { name: "Done" }).click();
  const line = block.getByRole("group", { name: /^Done today: / });
  await expect(line).toBeVisible();
  await expect(line).toContainText("Done today");
  await expect(group).toBeHidden();
  const dur = await block.locator(".t-fold").first().evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(dur.split(",").every((d) => parseFloat(d) === 0)).toBe(true);
  expect(await noScroll()).toBe(true);
  await shot(page, "home-collapsed", theme);
  await line.getByRole("button", { name: "Undo" }).click();
  await expect(group).toBeVisible();
  await expect(line).toBeHidden();
});

test("(done-fold) Home: a Done answer in the store is collapsed on load (derived, not component state)", async ({ page, theme }) => {
  await preview(page, { answer: { home: "done" } });
  await openApp(page, { theme, reduceMotion: true });
  const block = homeBlock(page);
  await expect(block.getByRole("group", { name: /^Done today: / })).toBeVisible();
  await expect(block.getByRole("group", { name: /Your answer/ })).toBeHidden();
  await page.reload();
  await expect(homeBlock(page).getByRole("group", { name: /^Done today: / })).toBeVisible();
});

test("(done-fold) full variant (Neuro): Done folds the whole block, with motion enabled the fold animates", async ({ page, theme }) => {
  await preview(page);
  await openApp(page, { theme, reduceMotion: false });
  const field = await openView(page, "Neuro");
  const full = field.locator(".t-block").first();
  await shot(page, "neuro-expanded", theme);
  await full.getByRole("group", { name: /Your answer/ }).getByRole("button", { name: "Done" }).click();
  const dur = await full.locator(".t-fold").first().evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(dur.split(",").some((d) => parseFloat(d) > 0)).toBe(true);
  await expect(full.getByRole("group", { name: /^Done today: / })).toBeVisible();
  await expect(full.getByRole("heading", { level: 3 })).toBeHidden();
  await page.waitForTimeout(400);
  await shot(page, "neuro-collapsed", theme);
  await full.getByRole("button", { name: "Undo" }).click();
  await expect(full.getByRole("heading", { level: 3 })).toBeVisible();
});
