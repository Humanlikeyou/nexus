import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, openApp, openView, routeSettled, test } from "./fixtures/tauri-mock";

/** design-1110d: Crews and Business command centers, with adapters unwired (the app) and with sample adapters (previews). */
const STATES: Array<[string, string, (p: Page) => Promise<void>]> = [
  ["Crews", "Crews", async () => {}],
  ["Crew run", "Crews", async (p) => { await activeField(p, "Agents").getByRole("row", { name: "Open run Sample research" }).click(); }],
  ["Crew approvals", "Crews", async (p) => { const b = activeField(p, "Agents").getByRole("button", { name: "View all ›" }); if (await b.count()) await b.click(); }],
  ["Enterprise overview", "Overview", async () => {}],
  ["Ventures", "Ventures", async () => {}],
  ["Taiyo Skin", "Ventures", async (p) => { const b = activeField(p, "Enterprise").getByRole("button", { name: /^Taiyo Skin/ }).first(); if (await b.count()) await b.click(); }],
  ["Finances", "Finances", async () => {}],
  // design-1111v: the new Enterprise tabs.
  ["Decisions", "Decisions", async () => {}],
  ["Tasks", "Tasks", async () => {}],
  ["Content", "Content", async () => {}],
];

async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}

for (const sample of [false, true]) {
  for (const [name, view, go] of STATES) {
    test(`(1110d) ${name}${sample ? " with sample adapters" : ""}: axe clean (incl. 4.5:1 contrast), no yen`, async ({ page, theme }) => {
      if (sample) await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
      await openApp(page, { theme, reduceMotion: true });
      await openView(page, view);
      await routeSettled(page).catch(() => undefined);
      await go(page);
      await page.waitForTimeout(300);
      expect(await blocking(page)).toEqual([]);
      const text = await page.locator(".active-field").innerText();
      expect(text).not.toMatch(/¥/);
      if (!sample) expect(text).not.toContain("Sample data");
    });
  }
}

test("(1110d) Crews: n8n lives in Crews; crew approvals are not Terrance's", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const field = await openView(page, "Crews");
  await expect(field.getByRole("region", { name: "n8n workflows" })).toBeVisible();
  await expect(field.getByRole("region", { name: "Crew approvals" })).toBeVisible();
});
