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
    document.body.append(probe);
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
  const { gap, color, secondary } = await page.evaluate(() => {
    // Visible (ink) gap: bottom of the AMARA strokes → cap height of "Command Center".
    const svg = document.querySelector(".splash svg.wordmark--splash") as SVGSVGElement;
    const box = svg.getBoundingClientRect();
    const vb = svg.viewBox.baseVal;
    const shapes = [...svg.querySelectorAll("path, line, polyline")] as SVGGraphicsElement[];
    const maxY = Math.max(...shapes.map((el) => { const b = el.getBBox(); return b.y + b.height; }));
    const stroke = parseFloat(getComputedStyle(shapes[0]).strokeWidth);
    const inkBottom = box.top + (maxY + stroke / 2 - vb.y) * (box.height / vb.height);
    const s = document.querySelector(".splash__subtitle") as HTMLElement;
    const cs = getComputedStyle(s);
    const ctx = document.createElement("canvas").getContext("2d")!;
    ctx.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
    const m = ctx.measureText("Command Center");
    const halfLeading = (parseFloat(cs.lineHeight) - (m.fontBoundingBoxAscent + m.fontBoundingBoxDescent)) / 2;
    const inkTop = s.getBoundingClientRect().top + halfLeading + m.fontBoundingBoxAscent - m.actualBoundingBoxAscent;
    const probe = document.createElement("span");
    probe.style.color = "var(--label-secondary)";
    document.body.append(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return { gap: inkTop - inkBottom, color: cs.color, secondary: c };
  });
  expect(color).toBe(secondary);
  expect(Math.abs(gap - 16)).toBeLessThanOrEqual(0.5);
  await expect(splash).toBeHidden();
});
