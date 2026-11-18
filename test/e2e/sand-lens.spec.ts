import AxeBuilder from "@axe-core/playwright";
import { expect } from "@playwright/test";
import { openApp, setMock, test } from "./fixtures/tauri-mock";

/** design-1118b: the sand lens on Home (opt-in here; other specs keep the classic lens on the box's software GL). */
test.describe("sand lens", () => {
  test.beforeEach(async ({ page }) => { await page.addInitScript(() => localStorage.setItem("nexus.sandTier", "3")); });

  test("draws nothing where the lens was: invisible hit area, click-through canvas, no old lens", async ({ page, theme }) => {
    await openApp(page, { theme, lens: "sand" });
    const hit = page.getByTestId("lens-hit");
    await expect(hit).toBeAttached();
    const box = await hit.boundingBox();
    expect(box && box.width).toBeGreaterThan(100);
    const canvas = page.getByTestId("sand-canvas");
    await expect(canvas).toBeAttached();
    expect(await canvas.evaluate((c) => getComputedStyle(c).pointerEvents)).toBe("none");
    await expect(page.locator(".landing .amara-lens")).toHaveCount(0);
    // the full-viewport canvas never blocks a click: the composer under it still takes focus
    const composer = page.getByRole("textbox", { name: /Message Amara/i }).first();
    await composer.click();
    await expect(composer).toBeFocused();
    // the topmost element at the lens centre is the hit area, not the canvas
    const top = await page.evaluate(({ x, y }) => (document.elementFromPoint(x, y) as HTMLElement | null)?.dataset.testid ?? null, { x: box!.x + box!.width / 2, y: box!.y + box!.height / 2 });
    expect(top).toBe("lens-hit");
  });

  test("voice off: the sand keeps floating where the lens is (nothing falls onto the chat card)", async ({ page, theme }) => {
    await openApp(page, { theme, lens: "sand" });
    await setMock(page, "voice_status", { installed: true, running: false, state: "idle", pid: 1, state_age_s: 2, voice: "neosoul_g", rss_mb: 1, launchd: false, dir: "/Users/taisun/projects/amara-voice" });
    await expect(page.locator(".landing__line")).toContainText("off", { timeout: 8000 });
    await expect(page.getByTestId("lens-hit")).toHaveAttribute("data-sand-state", "off");
    await expect(page.getByTestId("sand-canvas")).toBeAttached();
  });

  test("classic lens stays selectable in Settings", async ({ page, theme }) => {
    await openApp(page, { theme, lens: "classic" });
    await expect(page.locator(".landing .amara-lens")).toHaveCount(1);
    await expect(page.getByTestId("sand-canvas")).toHaveCount(0);
  });

  test("Home with the sand lens: no serious or critical axe violations", async ({ page, theme }) => {
    await openApp(page, { theme, lens: "sand" });
    await page.waitForTimeout(800);
    const r = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa"]).analyze();
    expect(r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);
  });

  test("canopy sunlight is a click-through layer on Home and is still with reduced motion", async ({ page, theme }) => {
    await openApp(page, { theme, lens: "sand", reduceMotion: true });
    const light = page.getByTestId("canopy-light");
    await expect(light).toBeAttached();
    await expect(light).toHaveAttribute("aria-hidden", "true");
    expect(await light.evaluate((el) => getComputedStyle(el).pointerEvents)).toBe("none");
    // reduced motion → static (data-calm)
    await expect(light).toHaveAttribute("data-calm", "true");
  });
});
