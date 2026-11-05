import { readFileSync } from "node:fs";
import { expect, openApp, openToday, test } from "./fixtures/tauri-mock";

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

test("glass applied: palette dialog blurs, cards stay solid, vibrancy layers transparent", async ({ page, theme }) => {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-transparency", value: "no-preference" }, { name: "prefers-color-scheme", value: theme }] });
  await openToday(page, { theme, reduceMotion: false });
  await page.keyboard.press("Meta+k");
  const dialog = page.locator("dialog.ui-dialog[open]").first();
  await expect(dialog).toBeVisible();
  const bf = await dialog.evaluate((el) => {
    const s = getComputedStyle(el); return s.backdropFilter || (s as unknown as Record<string, string>).webkitBackdropFilter;
  });
  expect(await page.evaluate(() => matchMedia("(prefers-reduced-transparency: reduce)").matches)).toBe(false);
  expect(bf).not.toBe("none");
  const css = readFileSync("src-console/styles/glass-apply.css", "utf8");
  expect(css).toMatch(/:root \.ui-dialog \{[^}]*backdrop-filter: blur\(var\(--blur-thick\)\)/);
  expect(css).toMatch(/:root \.context-inspector \{[^}]*backdrop-filter: blur/);
  const shots = process.env.GLASS_SHOTS;
  if (shots) await page.screenshot({ path: `${shots}/palette-${theme}.png` });
  await page.keyboard.press("Escape");
  const cardBf = await page.locator(".ui-card").first().evaluate((el) => getComputedStyle(el).backdropFilter).catch(() => "none");
  expect(cardBf).toBe("none");
  await page.evaluate(() => document.documentElement.setAttribute("data-vibrancy", "true"));
  for (const sel of ["html", "#root", "main.command-surface > nav.system-constellation"]) {
    expect(await page.locator(sel).first().evaluate((el) => getComputedStyle(el).backgroundColor), sel).toBe("rgba(0, 0, 0, 0)");
  }
});

test("reduced motion: dialog has no transform animation or transition", async ({ page, theme }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openApp(page, { theme, reduceMotion: true });
  await page.keyboard.press("Meta+k");
  const dialog = page.locator("dialog.ui-dialog[open]").first();
  await expect(dialog).toBeVisible();
  const s = await dialog.evaluate((el) => {
    const c = getComputedStyle(el); return { anim: c.animationName, transform: c.transform, tp: c.transitionProperty, td: c.transitionDuration };
  });
  expect(s.anim).toBe("none");
  expect(s.transform).toBe("none");
  expect(s.tp.includes("transform") && s.td !== "0s" && !s.td.startsWith("1e-05")).toBe(false);
  const shots = process.env.GLASS_SHOTS;
  if (shots) {
    await page.screenshot({ path: `${shots}/dialog-${theme}.png` });
    await page.keyboard.press("Escape");
    await page.keyboard.press("Tab");
    await page.keyboard.press("Tab");
    await page.screenshot({ path: `${shots}/focus-${theme}.png` });
  }
});

test("glass renders with reduced transparency forced off (review shot)", async ({ page, theme }) => {
  // Playwright's emulateMedia has no reducedTransparency option; force it via CDP (Chromium only).
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-transparency", value: "no-preference" }, { name: "prefers-color-scheme", value: theme }] });
  await openApp(page, { theme, reduceMotion: false /* reduced motion drops blur too */ });
  expect(await page.evaluate(() => matchMedia("(prefers-reduced-transparency: reduce)").matches)).toBe(false);
  await page.keyboard.press("Meta+k");
  const dialog = page.locator("dialog.ui-dialog[open]").first();
  await expect(dialog).toBeVisible();
  const bf = await dialog.evaluate((el) => getComputedStyle(el).backdropFilter);
  expect(bf).not.toBe("none");
  const shots = process.env.GLASS_SHOTS;
  if (shots && theme === "dark") {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.screenshot({ path: `${shots}/home-palette-dark.png` });
    console.log(`GLASS_BF=${bf}`);
  }
});

test("reduced transparency: shell chrome and composer have no blur", async ({ page, theme }) => {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-transparency", value: "reduce" }, { name: "prefers-color-scheme", value: theme }] });
  await openToday(page, { theme, reduceMotion: true });
  expect(await page.evaluate(() => matchMedia("(prefers-reduced-transparency: reduce)").matches)).toBe(true);
  for (const sel of ["main.command-surface > .intelligence-overlay > .nexus-mark", "main.command-surface > .intelligence-overlay > .field-clock", "main.command-surface > nav.system-constellation"]) {
    expect(await page.locator(sel).first().evaluate((el) => getComputedStyle(el).backdropFilter), sel).toBe("none");
  }
});
