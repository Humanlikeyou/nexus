import { expect } from "@playwright/test";
import { openApp, test } from "./fixtures/tauri-mock";

type SandHook = { engine: { freeze: (on?: boolean) => void; advance: (n: number, dt?: number) => void } | null; setState: (s: string | null) => void; tone: (mode: string, loud?: number) => void };

/**
 * Decision 0045 item 1: the sand lens has listen, think and speak states.
 * - The shaders must compile. three.js reports a GLSL error as "THREE.WebGLProgram: Shader Error".
 * - Each state must draw without a GL error.
 * - With SAND_SHOTS=1 it also saves a frame per state for Taisun's approval.
 */
const shots = !!globalThis.process?.env?.SAND_SHOTS;

test.describe("sand lens states", () => {
  test.beforeEach(async ({ page }) => { await page.addInitScript(() => localStorage.setItem("nexus.sandTier", "3")); });

  test("listen, think and speak compile and draw", async ({ page, theme }, info) => {
    if (shots) test.setTimeout(600_000);
    const glErrors: string[] = [];
    page.on("console", (m) => {
      const t = m.text();
      if (/Shader Error|WebGLProgram|GL_INVALID|falling back to the static lens/i.test(t)) glErrors.push(t.slice(0, 400));
    });
    await openApp(page, { theme, lens: "sand" });
    await expect(page.getByTestId("sand-canvas")).toBeAttached();
    await page.waitForFunction(() => !!(window as unknown as { __sand?: { engine: unknown } }).__sand?.engine, null, { timeout: 15_000 });
    for (const st of ["listen", "think", "speak"] as const) {
      await page.evaluate((s) => {
        const sand = (window as unknown as { __sand?: SandHook }).__sand!;
        sand.setState(s);
        if (s === "speak") sand.tone("n2m2", 0.85);
      }, st);
      if (shots) {
        // capture only: step the simulation by hand (software GL is too slow for real time)
        await page.evaluate(async (s) => {
          const sand = (window as unknown as { __sand?: SandHook }).__sand!;
          sand.engine!.freeze(true);
          for (let i = 0; i < (s === "think" ? 240 : 150); i++) {
            if (s === "speak" && i % 15 === 0) sand.tone(i % 30 === 0 ? "n2m2" : "n1m3", 0.6 + 0.3 * Math.sin(i / 7));
            sand.engine!.advance(1, 1 / 30);
            if (i % 10 === 0) await new Promise((r) => setTimeout(r, 0));
          }
        }, st);
      } else {
        await page.waitForTimeout(1200);
      }
      if (shots) {
        await page.getByTestId("sand-canvas").screenshot({ path: info.outputPath(`sand-${st}-${theme}.png`) });
      }
    }
    expect(glErrors, glErrors.join("\n")).toEqual([]);
  });
});
