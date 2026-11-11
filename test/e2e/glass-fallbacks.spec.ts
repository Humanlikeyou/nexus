import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { emulateDisplay as emulate, expect, openApp, openToday, openView, reducedTransparency, test, type Theme, openMore } from "./fixtures/tauri-mock";

// Glass fallbacks and contrast (ported from the Oct 30 /tmp/glass review specs).
const SECTIONS = ["Today", "Agents", "Mind–Body", "Fitness", "Neuro", "Business", "Settings"];

async function openSection(page: Page, theme: Theme, label: string) {
  await openApp(page, { theme, reduceMotion: false });
  const field = await openView(page, label);
  await expect(field.getByRole("heading", { level: 1 })).toBeVisible();
  await page.waitForTimeout(300);
}

/** Every element that still has a backdrop-filter. */
const blurred = (page: Page) =>
  page.evaluate(() =>
    (Array.from(document.querySelectorAll("*")) as HTMLElement[])
      .filter((el) => (getComputedStyle(el).backdropFilter || "none") !== "none")
      .map((el) => el.tagName.toLowerCase() + "." + String(el.className).split(" ")[0]),
  );

for (const label of SECTIONS) {
  test(`reduced transparency: ${label} has no blur`, async ({ page, theme }) => {
    await emulate(page, theme, "reduce");
    await openSection(page, theme, label);
    expect(await reducedTransparency(page)).toBe(true);
    expect(await blurred(page)).toEqual([]);
  });

  test(`reduced motion: ${label} has no blur and no animation`, async ({ page, theme }) => {
    await emulate(page, theme, "no-preference", "reduce");
    await openSection(page, theme, label);
    expect(await blurred(page)).toEqual([]);
    const moving = await page.evaluate(() =>
      (Array.from(document.querySelectorAll("*")) as HTMLElement[])
        .filter((e) => {
          const c = getComputedStyle(e);
          return (c.animationName !== "none" && parseFloat(c.animationDuration) > 0.001) || parseFloat(c.transitionDuration) > 0.001;
        })
        .map((e) => e.tagName.toLowerCase() + "." + String(e.className).split(" ")[0]),
    );
    expect(moving).toEqual([]);
  });

  test(`axe color-contrast: ${label}`, async ({ page, theme }) => {
    await emulate(page, theme, "no-preference", "reduce");
    await openSection(page, theme, label);
    const r = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
    expect(r.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  });
}

test("Home chat composer: solid in every mode (nothing scrolls under it), so no fallback is needed", async ({ page, theme }) => {
  const composer = () => page.locator(".hchat form.chat-screen__composer").first();
  const bf = () => composer().evaluate((el) => getComputedStyle(el).backdropFilter);
  await emulate(page, theme, "no-preference");
  await openApp(page, { theme, reduceMotion: false });
  await expect(composer()).toBeVisible();
  expect(await bf()).toBe("none");
  await emulate(page, theme, "reduce");
  expect(await bf()).toBe("none");
  await emulate(page, theme, "no-preference", "reduce");
  expect(await bf()).toBe("none");
});

test("palette dialog: blurred by default, solid under reduced transparency and reduced motion", async ({ page, theme }) => {
  await emulate(page, theme, "no-preference");
  await openApp(page, { theme, reduceMotion: false });
  await page.keyboard.press("Meta+k");
  const dialog = page.locator("dialog.ui-dialog[open]").first();
  await expect(dialog).toBeVisible();
  const bf = () => dialog.evaluate((el) => getComputedStyle(el).backdropFilter);
  expect(await bf()).not.toBe("none");
  const r = await new AxeBuilder({ page }).include("dialog.ui-dialog[open]").withRules(["color-contrast"]).analyze();
  expect(r.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await emulate(page, theme, "reduce");
  expect(await bf()).toBe("none");
  await emulate(page, theme, "no-preference", "reduce");
  expect(await bf()).toBe("none");
});

test("worst-case backdrop: text on glass chrome is at least 4.5:1 (3:1 large)", async ({ page, theme }) => {
  // design-1110b A: the chat composer (the last blurred page chrome) moved to the solid Home chat, so
  // every section is measured: whatever still blurs must hold 4.5:1 over white, black and the wallpaper.
  await emulate(page, theme, "no-preference");
  await openApp(page, { theme, reduceMotion: false });
  const measure = () => page.evaluate(() => {
    const parse = (s: string) => (s.match(/[\d.]+/g) || []).map(Number);
    const lum = ([r, g, b]: number[]) => {
      const f = (c: number) => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
      return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
    };
    const over = (fg: number[], a: number, bg: number[]) => fg.slice(0, 3).map((c, i) => c * a + bg[i] * (1 - a));
    const ratio = (a: number[], b: number[]) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
    // White, black and the emerald wallpaper tone behind the glass.
    const BEHIND = [[255, 255, 255], [0, 0, 0], [15, 61, 46]];
    const fails: string[] = [];
    const glass = (Array.from(document.querySelectorAll("*")) as HTMLElement[])
      .filter((e) => getComputedStyle(e).backdropFilter !== "none" && e.getBoundingClientRect().width > 0);
    for (const g of glass) {
      const gb = parse(getComputedStyle(g).backgroundColor);
      const ga = gb[3] ?? 1;
      for (const t of Array.from(g.querySelectorAll("*")) as HTMLElement[]) {
        const hasText = [...t.childNodes].some((n) => n.nodeType === 3 && n.textContent!.trim());
        if (!hasText || t.getBoundingClientRect().width === 0) continue;
        const c = getComputedStyle(t);
        const fc = parse(c.color);
        const op = Number(c.opacity);
        const fa = (fc[3] ?? 1) * (op === 0 ? 1 : op); // opacity 0 = hover-revealed hint: measure it as shown
        const size = parseFloat(c.fontSize);
        const need = size >= 24 || (Number(c.fontWeight) >= 700 && size >= 18.66) ? 3 : 4.5;
        const worst = Math.min(...BEHIND.map((b) => { const bg = over(gb, ga, b); return ratio(over(fc, fa, bg), bg); }));
        if (worst < need) fails.push(`${g.className.split(" ")[0]} > "${t.textContent!.trim().slice(0, 20)}" ${worst.toFixed(2)}`);
      }
    }
    return { surfaces: glass.length, fails };
  });
  const fails: string[] = [];
  for (const label of SECTIONS) {
    await openView(page, label);
    await page.waitForTimeout(300);
    fails.push(...(await measure()).fails.map((f) => `${label}: ${f}`));
  }
  expect(fails).toEqual([]);
});

test("segmented control: neutral selected pill, focus ring separated from it", async ({ page, theme }) => {
  await emulate(page, theme, "no-preference");
  await openSection(page, theme, "Agents");
  const group = page.getByRole("radiogroup").first();
  const checked = group.locator(".ui-segmented__segment[data-checked]");
  const label = await page.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.color = "var(--label)";
    document.body.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  });
  await expect(checked).toHaveCSS("color", label);
  await checked.focus();
  await page.keyboard.press("ArrowRight");
  const focused = group.locator(".ui-segmented__segment:focus-visible");
  await expect(focused).toHaveCount(1);
  await expect(focused).toHaveCSS("outline-style", "solid");
  // design-1110g: the segment inset moved onto the Fibonacci scale (2px → 3px), so the ring sits 3px out.
  await expect(focused).toHaveCSS("outline-offset", "3px");
});

test("settings: Allow Claude is off by default, notes data leaves the Mac, turns on", async ({ page, theme }) => {
  await openSection(page, theme, "Settings");
  await openMore(page);
  const toggle = page.getByRole("switch", { name: "Allow Claude" });
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(page.getByText("This sends data off the Mac.", { exact: false })).toBeVisible();
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
});
