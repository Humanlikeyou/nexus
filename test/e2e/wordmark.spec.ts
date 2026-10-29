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
