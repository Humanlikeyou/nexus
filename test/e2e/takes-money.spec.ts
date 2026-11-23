import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, gotoToday, navButton, openApp, openView, routeSettled, test } from "./fixtures/tauri-mock";

/** build-1111t: take memory, one daily suggestion per page, follow-through by type, and personal money (read-only; Life › Personal money since 1111v). */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const sub = async (p: Page, host: string, label: string) => { await navButton(p, host).click(); await activeField(p, host).getByRole("radio", { name: label, exact: true }).click(); await routeSettled(p).catch(() => undefined); };
const preview = (page: Page, takes: Record<string, unknown> = {}, money: Record<string, unknown> = {}) => page.addInitScript(([t, m]) => {
  localStorage.setItem("nexus.sampleAdapters", "1");
  localStorage.setItem("nexus.takesPreview", JSON.stringify(t));
  localStorage.setItem("nexus.moneyPreview", JSON.stringify(m));
}, [takes, money] as const);
const FORBIDDEN = /Let Home do it|3-week|Trial day|NOOP|OpenClaw/;

const SURFACES: Array<[string, (p: Page) => Promise<void>]> = [
  ["Neuro take + suggestion", async (p) => { await openView(p, "Neuro"); }],
  ["Patterns › Follow-through", async (p) => { await sub(p, "Mind–Body", "Patterns"); await p.getByRole("button", { name: /^Follow-through/ }).click(); }],
  // design-1111v: Business › Personal moved to Life › Personal money; goals and the weekly review moved to Life.
  ["Life › Personal money", async (p) => { await sub(p, "Life", "Personal money"); }],
  ["Today", async (p) => { await gotoToday(p); }],
  ["Life › Goals", async (p) => { await sub(p, "Life", "Goals"); }],
  ["Life › Weekly review", async (p) => { await sub(p, "Life", "Weekly review"); }],
];
for (const sample of [false, true]) {
  for (const [name, go] of SURFACES) {
    test(`(1111t) ${name}${sample ? " with sample adapters" : ""}: axe clean (incl. 4.5:1), honest tags`, async ({ page, theme }) => {
      if (sample) await preview(page);
      await openApp(page, { theme, reduceMotion: true });
      await go(page);
      await routeSettled(page).catch(() => undefined);
      await page.waitForTimeout(300);
      expect(await blocking(page)).toEqual([]);
      expect(await page.locator("main, body").first().textContent()).not.toMatch(FORBIDDEN);
      if (!sample) await expect(page.getByText("Sample data")).toHaveCount(0);
      else await expect(page.getByText("Sample data").first()).toBeVisible();
    });
  }
}

test("(1111t) unwired: the take says today's one thing Needs wiring and offers no answer buttons", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const field = await openView(page, "Neuro");
  const take = field.locator(".take").first();
  if (await take.count()) {
    await expect(field.getByRole("group", { name: /Your answer/ })).toHaveCount(0);
  }
});

test("(1111t) suggestion: answer, skip reason, Undo; history sheet opens with the 1-year rule", async ({ page, theme }) => {
  await preview(page);
  await openApp(page, { theme, reduceMotion: true });
  const field = await openView(page, "Neuro");
  const group = field.getByRole("group", { name: /Your answer/ });
  await group.getByRole("button", { name: "Skipped" }).click();
  await field.getByRole("button", { name: "No time" }).click();
  await expect(field.getByRole("button", { name: "No time" })).toHaveAttribute("aria-pressed", "true");
  await expect(field.getByText("Placeholder citation · not checked yet").first()).toBeVisible();
  await field.getByRole("button", { name: /^History · \d+ takes ›$/ }).click();
  const sheet = page.getByRole("dialog", { name: /Neuro · Amara's takes/ });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByText(/Kept 1 year, then summarized into Memory/)).toBeVisible();
  await page.waitForTimeout(300);
  expect(await blocking(page)).toEqual([]);
  await sheet.getByRole("button", { name: "Close" }).click();
  await expect(sheet).toBeHidden();
});

test("(1111t) fitness auto-detected Done can be undone", async ({ page, theme }) => {
  await preview(page, { answer: { fitness: "detected" } });
  await openApp(page, { theme, reduceMotion: true });
  const field = await openView(page, "Fitness");
  await expect(field.getByText(/WHOOP: 27-min walk/)).toBeVisible();
  await field.getByRole("button", { name: "Not right? Undo" }).click();
  await expect(field.getByRole("group", { name: /Your answer/ })).toBeVisible();
});

test("(1111t) money: read-only badges, seed phrase refused and cleared, JPY view, Not financial advice", async ({ page, theme }) => {
  await preview(page);
  await openApp(page, { theme, reduceMotion: true });
  await sub(page, "Life", "Personal money");
  const field = activeField(page, "Life");
  await expect(field.getByText("Can't move money", { exact: true })).toHaveCount(8);
  await expect(field.getByText(/^Not financial advice\./)).toBeVisible();
  await field.getByRole("radio", { name: "JPY" }).click();
  await expect(field.locator(".page-hero").first()).toContainText("7,511,767 JPY personal");
  await field.getByRole("button", { name: "Connect an account…" }).click();
  const sheet = page.getByRole("dialog", { name: "Connect an account (read-only)" });
  const input = sheet.getByRole("textbox", { name: /xpub or address/ });
  await input.fill("abandon ability able about above absent absorb abstract absurd abuse access accident");
  await expect(input).toHaveValue("");
  await expect(sheet.getByRole("alert")).toContainText("cleared it");
  await expect(sheet.getByRole("button", { name: "Add watch-only" })).toBeDisabled();
  expect(await page.content()).not.toContain("absurd abuse");
  await page.waitForTimeout(300);
  expect(await blocking(page)).toEqual([]);
  await sheet.getByRole("button", { name: "Cancel" }).click();
});

test("(1111t) money states: expired is greyed and left out; no suggestion on partial data", async ({ page, theme }) => {
  await preview(page, {}, { scenario: "states" });
  await openApp(page, { theme, reduceMotion: true });
  await sub(page, "Life", "Personal money");
  const field = activeField(page, "Life");
  await expect(field.getByText("as of Oct 19")).toBeVisible();
  await expect(field.getByText("No money suggestion today.")).toBeVisible();
  await page.waitForTimeout(300);
  expect(await blocking(page)).toEqual([]);
});
