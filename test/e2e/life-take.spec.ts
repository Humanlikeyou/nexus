import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, routeSettled, test } from "./fixtures/tauri-mock";

/** design-1111w added scope: Life pages show Amara's take (same AmaraTake card as other sections). */
const sample = (page: Page) => page.addInitScript(() => { localStorage.setItem("nexus.sampleAdapters", "1"); });
const settle = async (p: Page) => { await routeSettled(p).catch(() => undefined); await p.waitForTimeout(300); };
const lifeTab = async (p: Page, label: string) => {
  await navButton(p, "Life").click(); await settle(p);
  const r = activeField(p, "Life").getByRole("radio", { name: label, exact: true });
  if ((await r.getAttribute("aria-checked")) !== "true") await r.click();
  await settle(p);
  return activeField(p, "Life");
};
const lifeTake = (host: ReturnType<typeof activeField>) => host.locator(".life__take").getByRole("region", { name: /^Amara's take$/ });

test("Life Goals, Weekly review, Dates and Personal money each show Amara's take", async ({ page, theme, consoleErrors }) => {
  await sample(page);
  await openApp(page, { theme, reduceMotion: true });
  for (const label of ["Goals", "Weekly review", "Dates & deadlines", "Personal money"] as const) {
    const field = await lifeTab(page, label);
    await expect(field.getByRole("heading", { level: 1, name: label === "Dates & deadlines" ? "Dates & deadlines" : label === "Personal money" ? "Personal money" : label })).toBeVisible();
    const take = lifeTake(field);
    await expect(take).toBeVisible();
    await expect(field.getByRole("region", { name: /^Amara's take$/ })).toHaveCount(1);
    await expect(take).toHaveAttribute("data-state", "fresh");
    await expect(take).toContainText("Sample data");
    await expect(take).toContainText("4 of 5");
    if (label === "Personal money") {
      await expect(take).toContainText("5.5 months");
      await expect(take).toContainText("Today's one thing · Money");
      await expect(take).not.toContainText("Today's one thing · Life");
      await expect(take).toContainText("Not financial advice");
    }
  }
  expect(consoleErrors).toEqual([]);
});

// Expectation updated for two intentional changes, not weakened:
//  - 686340d gave Life takes live goal facts (goal_<id>: the roadmap's own %, or "current of target unit") from the live
//    goals the backend returns. The fixture's read_goals (snapshot.ts: "Run 300 km", 212 of 300) is live, not sample, so the
//    take is now "fresh" from those facts instead of "unwired"/"Needs wiring" (4e5ad29 also dropped the tag where data is live).
//  - 4e5ad29 hid the empty "Today's one thing" box on Life.
// The "invents no numbers" guarantee is kept and made stricter: no Sample data tag, none of the sample take's numbers, and
// every number in the take text must come from the fixture's live goal (212 and 300).
test("without sample adapters, the Life take is built only from live goal facts and invents no numbers", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const field = await lifeTab(page, "Goals");
  const take = lifeTake(field);
  await expect(take).toBeVisible();
  await expect(take).toHaveAttribute("data-state", "fresh");
  await expect(take).not.toContainText("Sample data");
  await expect(take).not.toContainText("4 of 5");
  await expect(take).not.toContainText("Today's one thing");
  const numbers = ((await take.locator(".take__body").innerText()).match(/\d+(?:\.\d+)?/g)) ?? [];
  for (const n of numbers) expect(["212", "300"]).toContain(n);
});
