import { expect, openApp, test } from "./fixtures/tauri-mock";

const TOKENS = [
  "--glass-ultrathin", "--glass-thin", "--glass-regular", "--glass-thick", "--glass-solid",
  "--glass-hairline", "--glass-specular", "--glass-bg", "--glass-shadow",
  "--shadow-1", "--shadow-2", "--shadow-3",
];

test("glass and shadow tokens resolve", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const vals = await page.evaluate((names) => {
    const s = getComputedStyle(document.documentElement);
    return Object.fromEntries(names.map((n) => [n, s.getPropertyValue(n).trim()]));
  }, TOKENS);
  for (const n of TOKENS) expect(vals[n], n).not.toBe("");
  const shots = process.env.GLASS_SHOTS;
  if (shots) {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.screenshot({ path: `${shots}/home-${theme}.png` });
  }
});
