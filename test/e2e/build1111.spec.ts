import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, gotoToday, navButton, openApp, openView, routeSettled, test } from "./fixtures/tauri-mock";

/** build-1111: Amara's take (1110f), the crew Files and terminal switch (1110i) and the personal OS (1111a). */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const sub = async (p: Page, host: string, label: string) => { await navButton(p, host).click(); await activeField(p, host).getByRole("radio", { name: label, exact: true }).click(); await routeSettled(p).catch(() => undefined); };
const SURFACES: Array<[string, (p: Page) => Promise<void>]> = [
  ["Home take", async () => {}],
  ["Fitness take", async (p) => { await openView(p, "Fitness"); }],
  ["Business take", async (p) => { await navButton(p, "Business").click(); }],
  ["Crews list", async (p) => { await openView(p, "Crews"); }],
  ["Crew detail", async (p) => { await openView(p, "Crews"); await p.getByRole("button", { name: /^(Manage|Review) smoke/ }).click(); }],
  ["Today sections", async (p) => { await gotoToday(p); }],
  ["Patterns", async (p) => { await sub(p, "Mind–Body", "Patterns"); }],
  ["Personal money", async (p) => { await sub(p, "Business", "Personal"); }],
  ["Settings memory", async (p) => { await navButton(p, "Settings").click(); }],
];

for (const sample of [false, true]) {
  for (const [name, go] of SURFACES) {
    test(`(1111) ${name}${sample ? " with sample adapters" : ""}: axe clean (incl. 4.5:1 contrast), honest tags`, async ({ page, theme }) => {
      if (sample) await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
      await openApp(page, { theme, reduceMotion: true });
      await go(page);
      await routeSettled(page).catch(() => undefined);
      await page.waitForTimeout(300);
      expect(await blocking(page)).toEqual([]);
      if (!sample) await expect(page.getByText("Sample data")).toHaveCount(0);
      else await expect(page.getByText("Sample data").first()).toBeVisible();
    });
  }
}

test("(1110f) Ask more opens Home chat with the take as context and the question pre-filled, nothing sent", async ({ page, theme }) => {
  await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true });
  await openView(page, "Fitness");
  await page.getByRole("button", { name: "Ask more ›" }).click();
  const chat = page.locator(".landing .hchat");
  await expect(chat.getByRole("note", { name: "Context for this question" })).toContainText("From Amara's take · Fitness");
  await expect(chat.getByRole("textbox", { name: "Message Amara" })).toHaveValue("Why is there room for a harder session today?");
  await chat.getByRole("button", { name: "Remove this context" }).click();
  await expect(chat.getByRole("note", { name: "Context for this question" })).toHaveCount(0);
});

test("(1110i) the switch turns on only after crew-service confirms; sends, spending, push and sudo stay blocked", async ({ page, theme }) => {
  await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true });
  await openView(page, "Crews");
  await page.getByRole("button", { name: /^Manage smoke/ }).click();
  const sw = page.getByRole("switch", { name: "Files and terminal for smoke" });
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await sw.click();
  await expect(page.getByRole("radio", { name: "1 hour" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("button", { name: "Turn On for 1 Hour" }).click();
  await expect(page.getByText("Applying…").first()).toBeVisible();
  await expect(sw).toHaveAttribute("aria-checked", "true", { timeout: 5000 });
  await expect(page.getByText("Enforced by crew-service")).toBeVisible();
  await expect(page.locator(".status-text[data-s=fail]", { hasText: "Blocked" })).toBeVisible();
  expect(await blocking(page)).toEqual([]);
});

test("(1111a) check-in: 5-step mood; Today shows goals, life admin and the weekly review; Memory is in Settings", async ({ page, theme }) => {
  await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true });
  const today = await gotoToday(page);
  await expect(today.getByRole("radiogroup", { name: "Mood" }).getByRole("radio")).toHaveCount(5);
  for (const id of ["checkin", "goals", "life-admin", "weekly-review"]) await expect(today.locator(`#${id}.page-section`)).toHaveCount(1);
  await navButton(page, "Settings").click();
  await expect(activeField(page, "Settings").locator("#memory.page-section")).toHaveCount(1);
});
