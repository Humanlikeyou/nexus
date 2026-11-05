import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, test, type Theme } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** Home landing page: the lens shows the real voice state. LANDING_SHOTS=<dir> saves screenshots (blur on). */
const STATE = "amara-voice://state";
const LEVEL = "amara-voice://level";
const SMALL = { width: 900, height: 620 };
const WIDE = { width: 1280, height: 800 };
const STATUS = responses().voice_status as Record<string, unknown>;
const voice = (over: Record<string, unknown>) => ({ ...responses(), voice_status: { ...STATUS, ...over } });
const waiting = voice({ state: "idle" });
const stopped = voice({ running: false, state: "stopped", pid: null, rss_mb: null });

const emit = (page: Page, event: string, payload: Record<string, unknown>) =>
  page.evaluate(([e, p]) => (window as unknown as { __E2E_EMIT: (e: unknown, p: unknown) => void }).__E2E_EMIT(e, { ts: Date.now(), ...p }), [event, payload] as const);
const lens = (page: Page) => page.locator('.amara-lens[data-size="320"]');
const line = (page: Page) => page.locator(".landing__line");
const NAV = ["Today", "Chat & Agents", "Mind–Body", "Business", "Settings", "Smart home"];

/** e2e Chromium reports reduced transparency; force it off so shots show the real look. */
async function realLook(page: Page, theme: Theme) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-transparency", value: "no-preference" }, { name: "prefers-color-scheme", value: theme }] });
}

test("Landing: launch opens the lens with no sidebar, toolbar or footer, and the buttons replace them", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, data: waiting });
  await expect(lens(page)).toBeVisible();
  await expect(page.locator("nav.system-constellation")).toHaveCount(0);
  await expect(page.locator(".field-clock")).toHaveCount(0);
  await expect(page.locator(".landing__nav").getByRole("button")).toHaveText(NAV);
  await expect(activeField(page, "Home").getByRole("heading", { level: 1 })).toHaveText("Command Center");
  expect(consoleErrors).toEqual([]);
});

test("Landing: launch ignores anything left in storage and lands on the lens", async ({ page, theme }) => {
  await page.addInitScript(() => { sessionStorage.setItem("amara.business.view", "revenue"); });
  await openApp(page, { theme, data: waiting });
  await expect(lens(page)).toBeVisible();
});

test("Landing: running and waiting for the wake word glows as listening, though the feed says idle", async ({ page, theme }) => {
  await openApp(page, { theme, data: waiting });
  await emit(page, STATE, { state: "idle" });
  await expect(lens(page)).toHaveAttribute("data-state", "listening");
  await expect(line(page)).toHaveText("Amara is listening");
});

test("Landing: thinking and speaking drive the big lens live", async ({ page, theme }) => {
  await openApp(page, { theme, data: waiting });
  await expect(lens(page)).toHaveAttribute("data-state", "listening");
  await emit(page, STATE, { state: "thinking" });
  await expect(lens(page)).toHaveAttribute("data-state", "thinking");
  await expect(line(page)).toHaveText("Amara is thinking");
  await emit(page, STATE, { state: "speaking", utteranceId: "u1" });
  await expect(line(page)).toHaveText("Amara is speaking");
  for (let i = 0; i < 12; i++) { await emit(page, LEVEL, { rms: 1, peak: 1, state: "speaking", utteranceId: "u1" }); await page.waitForTimeout(33); }
  await expect.poll(() => lens(page).evaluate((el) => Number((el as HTMLElement).style.getPropertyValue("--lens-scale")))).toBeGreaterThan(1.05);
  await emit(page, STATE, { state: "idle" });
  await expect(lens(page)).toHaveAttribute("data-state", "listening");
  await expect(page.getByTestId("amara-voice-announcer")).toHaveText("Amara is listening");
});

test("Landing: voice off is dim and says so, even if a stale feed says otherwise", async ({ page, theme }) => {
  await openApp(page, { theme, data: stopped });
  await expect(line(page)).toHaveText("Amara's voice is off");
  await expect(lens(page)).toHaveAttribute("data-state", "idle");
  await emit(page, STATE, { state: "speaking" });
  await page.waitForTimeout(100);
  await expect(lens(page)).toHaveAttribute("data-state", "idle");
  await emit(page, STATE, { state: "error", reason: "unavailable" });
  await expect(line(page)).toHaveText("Amara's voice is off");
  const dim = await lens(page).locator(".amara-lens__core").evaluate((el) => Number(getComputedStyle(el).opacity));
  expect(dim).toBeLessThan(.3);
});

test("Landing: other honest states", async ({ page, theme }) => {
  await openApp(page, { theme, data: voice({ installed: false, running: false, state: "not_installed" }) });
  await expect(line(page)).toHaveText("Amara Voice isn't set up");
  await expect(lens(page)).toHaveAttribute("data-state", "idle");
});

test("Landing: an unreachable status shows the error state, never a glow", async ({ page, theme }) => {
  await openApp(page, { theme, failing: ["voice_status"] });
  await expect(line(page)).toHaveText("Checking Amara Voice…");
  await expect(line(page)).toHaveText("Can't reach Amara Voice", { timeout: 5000 });
  await expect(lens(page)).toHaveAttribute("data-state", "error");
});

test("Landing: a failed turn shows error, and says she is still listening", async ({ page, theme }) => {
  await openApp(page, { theme, data: voice({ state: "error" }) });
  await expect(lens(page)).toHaveAttribute("data-state", "error");
  await expect(line(page)).toHaveText("Her last reply failed. Amara is still listening.");
});

test("Landing: Today opens the dashboard, Home returns to the lens, ⌘1 and ⌘K work", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, data: waiting });
  await navButton(page, "Today").click();
  const today = activeField(page, "Today");
  await expect(today.getByRole("heading", { level: 1 })).toHaveText("Command Center");
  await expect(today.locator(".home__summary")).toBeVisible();
  await expect(page.locator(".toolbar-location")).toHaveText("Today");
  await expect(lens(page)).toHaveCount(0);
  await navButton(page, "Home").click();
  await expect(lens(page)).toBeVisible();
  await page.keyboard.press("Meta+3");
  await expect(activeField(page, "Mind–Body").getByRole("heading", { level: 1 })).toHaveText("Mind–Body");
  await page.keyboard.press("Meta+1");
  await expect(lens(page)).toBeVisible();
  await page.keyboard.press("Meta+k");
  await page.getByRole("combobox", { name: "Search or ask Amara" }).fill("Home › Today");
  await page.keyboard.press("Enter");
  await expect(activeField(page, "Today").locator(".home__summary")).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test("Landing: every button is keyboard reachable with the emerald focus ring", async ({ page, theme }) => {
  await openApp(page, { theme, data: waiting });
  const buttons = page.locator(".landing__nav").getByRole("button");
  for (const name of NAV) {
    await page.keyboard.press("Tab");
    await expect(buttons.filter({ hasText: name })).toBeFocused();
  }
  const ring = await buttons.last().evaluate((el) => { const s = getComputedStyle(el); return `${s.outlineStyle} ${s.outlineWidth} ${s.outlineOffset} ${s.outlineColor}`; });
  expect(ring).toBe("solid 3px 2px rgb(52, 199, 154)");
  await page.keyboard.press("Enter");
  await expect(activeField(page, "Smart home")).toBeVisible();
});

test("Landing: fits the window at every size with no clipping or page scroll", async ({ page, theme }) => {
  await openApp(page, { theme, data: waiting });
  for (const [width, height, min] of [[1280, 800, 320], [1440, 900, 320], [1920, 1080, 320], [1024, 700, 240], [900, 620, 240], [820, 600, 220], [1600, 600, 240]] as const) {
    await page.setViewportSize({ width, height });
    const box = (await lens(page).boundingBox())!;
    expect(box.width, `${width}×${height}`).toBeGreaterThanOrEqual(min);
    expect(box.width, `${width}×${height}`).toBeLessThanOrEqual(320);
    expect(box.y, `${width}×${height}`).toBeGreaterThanOrEqual(0);
    expect(box.x, `${width}×${height}`).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width, `${width}×${height}`).toBeLessThanOrEqual(width);
    const scroll = await page.locator(".landing").evaluate((el) => ({ w: el.scrollWidth <= el.clientWidth, h: el.scrollHeight <= el.clientHeight }));
    expect(scroll, `${width}×${height}`).toEqual({ w: true, h: true });
    const last = (await page.locator(".landing__nav").getByRole("button").last().boundingBox())!;
    expect(last.y + last.height, `${width}×${height}`).toBeLessThanOrEqual(height);
    expect(last.x + last.width, `${width}×${height}`).toBeLessThanOrEqual(width);
  }
});

test("Landing: the six buttons sit in one centred row at both window sizes", async ({ page, theme }) => {
  await openApp(page, { theme, data: waiting });
  for (const viewport of [WIDE, SMALL]) {
    await page.setViewportSize(viewport);
    const boxes = await Promise.all((await page.locator(".landing__nav").getByRole("button").all()).map(async (b) => (await b.boundingBox())!));
    expect(boxes).toHaveLength(6);
    expect(new Set(boxes.map((b) => b.y)).size, `${viewport.width}: one row`).toBe(1);
    const left = boxes[0].x;
    const right = viewport.width - (boxes[5].x + boxes[5].width);
    expect(Math.abs(left - right), `${viewport.width}: centred`).toBeLessThanOrEqual(1);
    expect(left, `${viewport.width}: side margin`).toBeGreaterThanOrEqual(24);
    const gaps = boxes.slice(1).map((b, i) => Math.round(b.x - (boxes[i].x + boxes[i].width)));
    expect(new Set(gaps).size, `${viewport.width}: even gaps`).toBe(1);
  }
});

test("Landing: reduced motion keeps the honest glow, with nothing running", async ({ page, theme }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openApp(page, { theme, data: waiting });
  await expect(lens(page)).toHaveAttribute("data-state", "listening");
  expect(await lens(page).evaluate((el) => el.getAnimations({ subtree: true }).length)).toBe(0);
  const core = await lens(page).locator(".amara-lens__core").evaluate((el) => Number(getComputedStyle(el).opacity));
  expect(core).toBeGreaterThan(.9);
});

test("Landing: reduced transparency stays dim when voice is off", async ({ page, theme }) => {
  await openApp(page, { theme, data: stopped });
  await expect(line(page)).toHaveText("Amara's voice is off");
  const core = await lens(page).locator(".amara-lens__core").evaluate((el) => Number(getComputedStyle(el).opacity));
  expect(core).toBeLessThan(.3);
});

test("Landing: adds no blurred layer", async ({ page, theme }) => {
  await realLook(page, theme);
  await openApp(page, { theme, data: waiting });
  const blurred = await page.locator(".landing").evaluate((root) => [root, ...root.querySelectorAll("*")].filter((el) => {
    const s = getComputedStyle(el); return (s.backdropFilter || "none") !== "none";
  }).length);
  expect(blurred).toBe(0);
});

test("Landing: no serious or critical axe violations, listening or off", async ({ page, theme }) => {
  for (const data of [waiting, stopped]) {
    await openApp(page, { theme, data, reduceMotion: true });
    await expect(line(page)).not.toHaveText("Checking Amara Voice…");
    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);
  }
});

test("Landing: stays the dark design in light theme too", async ({ page, theme }) => {
  await openApp(page, { theme, data: waiting });
  await expect(line(page)).toHaveCSS("color", "rgb(244, 241, 234)");
  await expect(page.locator(".landing__wordmark")).toHaveCSS("color", "rgb(199, 154, 98)");
  expect(await page.locator(".landing").evaluate((el) => getComputedStyle(el).backgroundImage)).toContain("radial-gradient");
});

test.describe("sign-off shots", () => {
  test.skip(() => !process.env.LANDING_SHOTS, "LANDING_SHOTS not set");
  test("six buttons 1280x800", async ({ page, theme }) => {
    await realLook(page, theme);
    await page.setViewportSize(WIDE);
    await openApp(page, { theme, data: waiting });
    await expect(line(page)).toHaveText("Amara is listening");
    await page.waitForTimeout(1900);
    await page.screenshot({ path: `${process.env.LANDING_SHOTS}/landing-six-1280x800-${theme}.png` });
  });
  for (const [name, data] of [["listening", waiting], ["voice-off", stopped]] as const) {
    for (const [size, viewport] of [["1280x800", WIDE], ["900x620", SMALL]] as const) {
      test(`${name} ${size}`, async ({ page, theme }) => {
        await realLook(page, theme);
        await page.setViewportSize(viewport);
        await openApp(page, { theme, data });
        await expect(line(page)).toHaveText(name === "listening" ? "Amara is listening" : "Amara's voice is off");
        await page.waitForTimeout(1900); // the listening ring at the top of its slow breath
        await page.screenshot({ path: `${process.env.LANDING_SHOTS}/landing-${name}-${size}-${theme}.png` });
      });
    }
  }
});
