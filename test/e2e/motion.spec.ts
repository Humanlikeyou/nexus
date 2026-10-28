import { expect, openApp, test } from "./fixtures/tauri-mock";
import type { Page } from "@playwright/test";

/** Running CSS animations inside `selector`, as { name, durationMs, transform }. */
async function animationsIn(page: Page, selector: string) {
  return page.evaluate((sel) => {
    const root = document.querySelector(sel);
    if (!root) return null;
    return document.getAnimations()
      .filter((a) => {
        const t = (a.effect as KeyframeEffect | null)?.target;
        return t instanceof Element && (root === t || root.contains(t));
      })
      .map((a) => {
        const effect = a.effect as KeyframeEffect;
        const frames = effect.getKeyframes();
        return {
          name: (a as CSSAnimation).animationName ?? a.id,
          durationMs: Number(effect.getComputedTiming().duration) || 0,
          transform: frames.some((k) => k.transform != null && k.transform !== "none"),
        };
      });
  }, selector);
}

const SPLASH_DELAY = { get_terrence_runtime_health: 900 };

test.describe("(f) reduced motion (data-reduce-motion)", () => {
  test("splash and dialog animate opacity only (no transform)", async ({ page, theme }) => {
    await openApp(page, { theme, reduceMotion: true, delays: SPLASH_DELAY }).catch(() => {});
    await expect(page.locator("html")).toHaveAttribute("data-reduce-motion", "true");
    await expect(page.locator(".splash")).toBeVisible();
    const splash = await animationsIn(page, ".splash");
    expect(splash, "splash rendered").not.toBeNull();
    expect(splash!.filter((a) => a.transform)).toEqual([]);

    await expect(page.locator(".splash")).toBeHidden();
    await page.keyboard.press("Meta+k");
    await expect(page.locator(".ui-dialog[open]")).toBeVisible();
    const dialog = await animationsIn(page, ".ui-dialog[open]");
    expect(dialog!.filter((a) => a.transform)).toEqual([]);
  });

  // BUG (spec vs product): under reduced motion the splash keeps a 600 ms and the
  // dialog a 200 ms opacity fade (src-console/shell/Splash.css:7,
  // src-console/ui/Dialog.css:18 switch animation-name only; durations stay).
  // Spec (f) asks for no animation longer than 0 ms. Needs a product decision.
  test.fixme("no splash/dialog animation is longer than 0 ms", async ({ page, theme }) => {
    await openApp(page, { theme, reduceMotion: true, delays: SPLASH_DELAY }).catch(() => {});
    await expect(page.locator(".splash")).toBeVisible();
    const splash = await animationsIn(page, ".splash");
    expect(splash!.filter((a) => a.durationMs > 0)).toEqual([]);
    await expect(page.locator(".splash")).toBeHidden();
    await page.keyboard.press("Meta+k");
    await expect(page.locator(".ui-dialog[open]")).toBeVisible();
    const dialog = await animationsIn(page, ".ui-dialog[open]");
    expect(dialog!.filter((a) => a.durationMs > 0)).toEqual([]);
  });
});
