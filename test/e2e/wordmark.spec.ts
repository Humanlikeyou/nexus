import { expect, openApp, test } from "./fixtures/tauri-mock";

// screen-specs › Shell › Wordmark (Decision 17): drawn AMARA SVG, no font file, bronze per theme.
const BRONZE = { light: "rgb(140, 98, 57)", dark: "rgb(180, 134, 78)" };

test("(w) sidebar wordmark: 12px caps centred on the traffic lights, bronze, named Amara OS", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  const lockup = page.getByRole("img", { name: "Amara OS" }).first();
  await expect(lockup).toBeVisible();
  const svg = lockup.locator("svg.wordmark--sidebar");
  await expect(svg).toHaveCount(1);
  await expect(svg).toHaveCSS("color", BRONZE[theme]);
  const box = (await svg.boundingBox())!;
  // 7.94 × 12 wide, 1.14 × 12 tall (stroke overhang 0.84px each side around the 12px cap box).
  expect(box.width).toBeCloseTo(95.3, 0);
  expect(box.height).toBeCloseTo(13.7, 0);
  // Ink starts at x = 84 (viewBox pad 0.84px minus half the 1.5px stroke).
  expect(Math.round(box.x + 0.84 - 0.75)).toBe(84);
  // Cap box centre on the 12px traffic lights at y = 26 (±0.75px for the optical nudge).
  expect(Math.abs(box.y + box.height / 2 - 26)).toBeLessThanOrEqual(0.75);
  const os = lockup.locator(".wordmark-os");
  await expect(os).toHaveCSS("font-size", "11px");
  const osBox = (await os.boundingBox())!;
  expect(osBox.x + osBox.width).toBeLessThanOrEqual(220);
});

test("(w) no Cormorant / font file is requested for the wordmark", async ({ page, theme }) => {
  const fonts: string[] = [];
  page.on("request", (r) => { if (r.resourceType() === "font" || /\.woff2?$/.test(r.url())) fonts.push(r.url()); });
  await openApp(page, { theme, reduceMotion: true });
  await expect(page.getByRole("img", { name: "Amara OS" }).first()).toBeVisible();
  expect(fonts).toEqual([]);
});

const INACTIVE = { light: "rgb(145, 120, 95)", dark: "rgb(147, 125, 97)" };

test("(w) inactive window: toolbar mark dims to solid bronze, OS stays secondary, selected icon goes neutral", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  await expect(page.locator("html")).toHaveAttribute("data-window-active", "true");
  const lockup = page.getByRole("img", { name: "Amara OS" }).first();
  const os = lockup.locator(".wordmark-os");
  const icon = page.locator("main.command-surface .nav-group button.is-active > i").first();
  const osActive = await os.evaluate((el) => getComputedStyle(el).color);
  const iconActive = await icon.evaluate((el) => getComputedStyle(el).color);

  await page.evaluate(() => window.dispatchEvent(new Event("blur")));
  await expect(page.locator("html")).toHaveAttribute("data-window-active", "false");
  await expect(lockup.locator("svg.wordmark--sidebar")).toHaveCSS("color", INACTIVE[theme]);
  await expect(os).toHaveCSS("color", osActive);
  const secondary = await page.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.color = "var(--label-secondary)";
    // Neutral in the sidebar = the chrome's secondary label (shell.css, stronger over vibrancy).
    document.querySelector("main.command-surface > nav.system-constellation")!.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  });
  await expect(icon).toHaveCSS("color", secondary);
  expect(secondary).not.toBe(iconActive);

  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(lockup.locator("svg.wordmark--sidebar")).toHaveCSS("color", BRONZE[theme]);
  await expect(icon).toHaveCSS("color", iconActive);
});

test.describe("splash at 2x", () => {
  test.use({ deviceScaleFactor: 2 });
  test("(w) splash: \"Command Center\" subtitle 16px (visible) under the mark, SF 13px secondary, aria-hidden (Decision 13)", async ({ page, theme }) => {
    await openApp(page, { theme, reduceMotion: true, delays: { get_terrence_runtime_health: 900 } }).catch(() => {});
    const splash = page.locator(".splash");
    await expect(splash).toBeVisible();
    await expect(splash).toHaveAttribute("aria-label", "Amara OS is loading");
    const subtitle = splash.locator(".splash__subtitle");
    await expect(subtitle).toHaveText("Command Center");
    await expect(subtitle).toHaveAttribute("aria-hidden", "true");
    await expect(subtitle).toHaveCSS("font-size", "13px");
    await expect(subtitle).toHaveCSS("font-weight", "400");
    const { color, secondary } = await page.evaluate(() => {
      const probe = document.createElement("span");
      probe.style.color = "var(--label-secondary)";
      document.body.append(probe);
      const c = getComputedStyle(probe).color;
      probe.remove();
      return { color: getComputedStyle(document.querySelector(".splash__subtitle")!).color, secondary: c };
    });
    expect(color).toBe(secondary);
    // Visible gap, measured on rendered pixels (2x): blank rows between the AMARA strokes and the subtitle caps.
    const png = await page.locator(".splash__lockup").screenshot({ animations: "disabled" });
    const gap = await page.evaluate(async (b64) => {
      const img = new Image();
      img.src = `data:image/png;base64,${b64}`;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const ctx = c.getContext("2d")!;
      ctx.drawImage(img, 0, 0);
      const { data } = ctx.getImageData(0, 0, c.width, c.height);
      const bg = [data[0], data[1], data[2]];
      const inked = (y: number) => {
        for (let x = 0; x < c.width; x++) {
          const i = (y * c.width + x) * 4;
          if (Math.abs(data[i] - bg[0]) + Math.abs(data[i + 1] - bg[1]) + Math.abs(data[i + 2] - bg[2]) > 60) return true;
        }
        return false;
      };
      let y = 0;
      while (y < c.height && !inked(y)) y++; // top of the mark
      while (y < c.height && inked(y)) y++; // through the mark
      const start = y;
      while (y < c.height && !inked(y)) y++; // blank rows = the visible gap
      return (y - start) / (window.devicePixelRatio || 1);
    }, png.toString("base64"));
    expect(gap).toBeGreaterThanOrEqual(15.5);
    expect(gap).toBeLessThanOrEqual(16.5);
    await expect(splash).toBeHidden();
  });
});
