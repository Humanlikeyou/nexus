import { activeField, expect, openToday, test, openMore } from "./fixtures/tauri-mock";

// screen-specs › Home › Greeting (Decision 13): greeting, then status · "Thursday, Oct 29" from the app clock.
test("(h) Home subline: greeting, status from data, and today's date from the clock", async ({ page, theme }) => {
  await openToday(page, { theme, reduceMotion: true });
  const home = activeField(page, "Today");
  await expect(home.getByRole("heading", { level: 1 })).toHaveText("Command Center");
  const today = await page.evaluate(() => new Date(Date.now()).toLocaleDateString("en-US", { timeZone: "Asia/Tokyo", weekday: "long", month: "short", day: "numeric" }));
  expect(today).toMatch(/^[A-Z][a-z]+day, [A-Z][a-z]{2} \d{1,2}$/);
  const line = home.locator(".home__summary");
  await expect(line).toHaveText(new RegExp(`^(Amara has \\d+ actions? waiting for approval|Amara is tracking \\d+ priorit(y|ies) today|All clear\\. Amara is standing by\\.) · ${today}$`));
  await expect(home.locator(".page-hero__statement")).toHaveText(/^Good (morning|afternoon|evening), Taisun$/);
});

// screen-specs › Command center feel: one 28px status strip under the subline, plain text, no Badges.
test("(i) Home status strip: three text segments under the subline, deep links, no Badge", async ({ page, theme }) => {
  await openToday(page, { theme, reduceMotion: true });
  const home = activeField(page, "Today");
  await openMore(page);
  const strip = home.getByRole("group", { name: "Status" });
  await expect(strip).toBeVisible();
  const buttons = strip.getByRole("button");
  await expect(buttons).toHaveCount(3);
  await expect(buttons.nth(0)).toHaveText(/^Amara (online|offline)( · Assistant connected)?$/);
  await expect(buttons.nth(1)).toHaveText(/^(Next: (Consolidation|Reasoning|Daily summary) (in .+|due now)|Background jobs (stopped|running))$/);
  await expect(buttons.nth(2)).toHaveText(/^NOOP /);
  await expect(strip.locator(".ui-badge")).toHaveCount(0);
  const box = (await strip.boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(28);
  expect(box.height).toBeLessThan(40);
  const subline = (await home.locator(".home__summary").boundingBox())!;
  expect(box.y).toBeGreaterThan(subline.y);
  // Reduced motion: the dots are static.
  const anim = await strip.locator(".ui-status-dot__dot").first().evaluate((el) => getComputedStyle(el).animationName);
  expect(anim).toBe("none");
  await expect(home.getByText("Gateway online")).toHaveCount(0);
  await buttons.nth(2).click();
  await expect(activeField(page, "Mind–Body").getByRole("heading", { level: 1 })).toHaveText("Fitness");
});
