import { readFileSync } from "node:fs";
import { activeField, expect, openApp, test } from "./fixtures/tauri-mock";

// Emerald wash (design, Oct 29): content pane, content toolbar edge and Home header only.
// Under [data-vibrancy] html, #root, body, the sidebar and its header must stay transparent (6fe246e).
test("emerald wash never targets the vibrancy layers", () => {
  const css = readFileSync("src-console/styles/emerald.css", "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  const selectors = [...css.matchAll(/([^{}]+)\{/g)].map((m) => m[1]).join(",");
  expect(selectors).toContain(".active-field");
  for (const banned of ["#root", "body", "html", "system-constellation", "nexus-mark"]) expect(selectors).not.toContain(banned);
  expect(css).toMatch(/prefers-reduced-transparency: reduce/);
  expect(css).toMatch(/data-reduce-transparency="true"/);
});

test("emerald wash: vibrancy layers stay transparent, content pane gets the wash", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  await expect(activeField(page, "Home").getByRole("heading", { level: 1 })).toHaveText("Command Center");
  await page.evaluate(() => document.documentElement.setAttribute("data-vibrancy", "true"));
  const bg = (sel: string) => page.locator(sel).first().evaluate((el) => {
    const s = getComputedStyle(el); return `${s.backgroundColor}|${s.backgroundImage}`;
  });
  for (const sel of ["html", "#root", "main.command-surface > nav.system-constellation", "main.command-surface > .intelligence-overlay > .nexus-mark"]) {
    expect(await bg(sel), sel).toBe("rgba(0, 0, 0, 0)|none");
  }
  expect(await bg("main.command-surface > .active-field")).toContain("linear-gradient");
  const shots = process.env.EMERALD_SHOTS;
  if (shots) {
    await page.evaluate(() => document.documentElement.removeAttribute("data-vibrancy"));
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.screenshot({ path: `${shots}/home-${theme}.png` });
    for (const name of ["Chat", "Neuro"]) {
      await page.locator("nav.system-constellation").getByRole("button", { name: new RegExp(`^${name}`) }).first().click();
      await page.waitForTimeout(600);
      await page.screenshot({ path: `${shots}/${name.toLowerCase()}-${theme}.png` });
    }
  }
});
