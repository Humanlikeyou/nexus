import { activeField, expect, openApp, test } from "./fixtures/tauri-mock";

// screen-specs › Home › Greeting (Decision 13): greeting, then status · "Thursday, Oct 29" from the app clock.
test("(h) Home subline: greeting, status from data, and today's date from the clock", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const home = activeField(page, "Home");
  await expect(home.getByRole("heading", { level: 1 })).toHaveText("Command Center");
  const today = await page.evaluate(() => new Date(Date.now()).toLocaleDateString("en-US", { timeZone: "Asia/Tokyo", weekday: "long", month: "short", day: "numeric" }));
  expect(today).toMatch(/^[A-Z][a-z]+day, [A-Z][a-z]{2} \d{1,2}$/);
  const line = home.locator(".home__summary");
  await expect(line).toHaveText(new RegExp(`^(Amara has \\d+ actions? waiting for approval|Amara is tracking \\d+ priorit(y|ies) today|All clear\\. Amara is standing by\\.) · ${today}$`));
  await expect(home.locator(".home__date").first()).toHaveText(/^Good (morning|afternoon|evening), Taisun$/);
});
