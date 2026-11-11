import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, gotoToday, openApp, routeSettled, test } from "./fixtures/tauri-mock";

/** build-1111u: Home's take from the last 15 minutes of heart rate (no button) and Today's "Your body today" (option C). */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const preview = (page: Page, hr: Record<string, unknown> | null) => page.addInitScript((h) => {
  localStorage.setItem("nexus.sampleAdapters", "1");
  if (h) localStorage.setItem("nexus.hrTakePreview", JSON.stringify(h));
}, hr);
const homeTake = (page: Page) => page.locator(".landing__welcome").getByRole("region", { name: "Amara's take" });
const NO_BUTTON = /refresh|fresh take|regenerate|update take/i;

for (const sample of [false, true]) {
  test(`(1111u) Home and Today${sample ? " with sample adapters" : ""}: axe clean, honest tags, live 89-second trace kept, no refresh button`, async ({ page, theme, consoleErrors }) => {
    if (sample) await preview(page, { delayMs: 50 });
    await openApp(page, { theme, reduceMotion: true });
    const take = homeTake(page);
    await expect(take).toBeVisible();
    // Taisun 17:36: the landing keeps its live 89-second trace; the take does not draw a second chart there.
    await expect(page.locator(".landing__heart-rate")).toHaveAttribute("aria-label", /^Heart rate \d+ beats per minute$/);
    await expect(take.getByRole("img", { name: /^Heart rate, last 15 minutes/ })).toHaveCount(0);
    await expect(page.getByRole("button", { name: NO_BUTTON })).toHaveCount(0);
    if (sample) {
      await expect(take).toHaveAttribute("data-state", "fresh");
      await expect(take.getByText("Sample data")).toBeVisible();
      await expect(take).toContainText("Just now");
      await expect(take).toContainText("· WHOOP · 15 of 15 min");
    } else {
      await expect(take).toHaveAttribute("data-state", "unwired");
      await expect(take.getByText("Needs wiring", { exact: true })).toBeVisible();
      await expect(page.getByText("Sample data")).toHaveCount(0);
    }
    expect(await blocking(page)).toEqual([]);

    const field = await gotoToday(page);
    await routeSettled(page).catch(() => undefined);
    const card = field.getByRole("region", { name: "Your body today" });
    await expect(card).toBeVisible();
    await expect(field.getByRole("heading", { name: "Recovery & sleep" })).toHaveCount(0);
    await expect(card.getByRole("meter", { name: "Recovery" })).toBeVisible();
    await expect(card.getByLabel("Last night")).toContainText("Sleep");
    const inline = card.getByRole("region", { name: "Amara's take" });
    if (sample) {
      await expect(inline.getByRole("img", { name: /^Heart rate, last 15 minutes: \d+ to \d+, average \d+; your usual 62–74$/ })).toBeVisible();
      await expect(inline.getByText("Sample data")).toBeVisible();
    } else {
      await expect(inline.getByText("Needs wiring", { exact: true })).toBeVisible();
    }
    await expect(card.getByRole("button", { name: NO_BUTTON })).toHaveCount(0);
    expect(await blocking(page)).toEqual([]);
    expect(consoleErrors).toEqual([]);
  });
}

const STATES: Array<[string, Record<string, unknown>, string, RegExp]> = [
  ["current", { scenario: "current" }, "current", /as of 1:42 PM · 18 min ago/],
  ["regenerating", { scenario: "regenerating" }, "regenerating", /Reading your heart rate from 1:45–2:00 PM…/],
  ["new after 30+ min", { scenario: "new", delayMs: 50 }, "fresh", /Just now.*rose from 70 to 80.*At 1:13 PM you were at 68 and settling\./],
  ["not enough heart rate", { scenario: "not_enough", delayMs: 50 }, "not-enough", /Only 6 of the last 15 minutes have readings\. Amara writes once there are 10 or more\. The 1:13 PM take is kept\./],
  ["WHOOP unavailable", { scenario: "source_down", delayMs: 50 }, "source-down", /WHOOP hasn't synced since 12:33 PM, so Amara won't write a take from old data\./],
];
for (const [name, opts, state, text] of STATES) {
  test(`(1111u) Home take: ${name}`, async ({ page, theme }) => {
    await preview(page, opts);
    await openApp(page, { theme, reduceMotion: true });
    const take = homeTake(page);
    await expect(take).toHaveAttribute("data-state", state);
    await expect(take).toContainText(text);
    await expect(take.getByText("Sample data")).toBeVisible();
    if (state === "regenerating") await expect(take).toHaveAttribute("aria-busy", "true");
    expect(await blocking(page)).toEqual([]);
  });
}

test("(1111u) WHOOP unavailable links to Settings › Devices", async ({ page, theme }) => {
  await preview(page, { scenario: "source_down", delayMs: 10 });
  await openApp(page, { theme, reduceMotion: true });
  await homeTake(page).getByRole("button", { name: "Check WHOOP in Settings › Devices ›" }).click();
  await expect(activeField(page, "Settings").getByRole("radio", { name: "Devices", exact: true })).toHaveAttribute("aria-checked", "true");
});

test("(1111u) one take for Home and Today: navigating within 30 minutes doesn't write another", async ({ page, theme }) => {
  await preview(page, { delayMs: 50 });
  await openApp(page, { theme, reduceMotion: true });
  const take = homeTake(page);
  await expect(take).toHaveAttribute("data-state", "fresh");
  const first = await take.locator(".take__body").textContent();
  const field = await gotoToday(page);
  const inline = field.getByRole("region", { name: "Your body today" }).getByRole("region", { name: "Amara's take" });
  await expect(inline.locator(".take__body")).toHaveText(first ?? "");
  await expect(inline).not.toHaveAttribute("data-state", "regenerating");
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(inline.locator(".take__body")).toHaveText(first ?? "");
});
