import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, openView, test, openMore } from "./fixtures/tauri-mock";

const motion = (page: Page) => page.evaluate(() => document.documentElement.dataset.routeMotion ?? null);

/** Records every motion kind and any skeleton that shows up, from before the click. */
async function record(page: Page) {
  await page.evaluate(() => {
    const seen: string[] = [];
    (window as unknown as { __motion: string[] }).__motion = seen;
    new MutationObserver(() => {
      const kind = document.documentElement.dataset.routeMotion;
      if (kind) seen.push(kind);
      if (document.querySelector(".screen-skeleton")) seen.push("skeleton");
    }).observe(document.documentElement, { subtree: true, childList: true, attributes: true });
  });
}
const recorded = (page: Page) => page.evaluate(() => (window as unknown as { __motion: string[] }).__motion);


test.describe("motion kinds", () => {
  test("lens page to a section and back is one space, section to section is a section move, sub-views are their own", async ({ page, theme }) => {
    await openApp(page, { theme });
    await record(page);
    await navButton(page, "Mind–Body").click();
    await expect(activeField(page, "Mind–Body").getByRole("heading", { level: 1 })).toHaveText("Mind–Body");
    await activeField(page, "Mind–Body").getByRole("radio", { name: "Fitness", exact: true }).click();
    await expect(activeField(page, "Mind–Body").getByRole("heading", { level: 1 })).toHaveText("Fitness");
    await navButton(page, "Settings").click();
    await expect(activeField(page, "Settings").getByRole("heading", { level: 1 })).toHaveText("Settings");
    await navButton(page, "Home").click();
    await expect(page.locator(".landing")).toBeVisible();
    await expect.poll(() => motion(page)).toBeNull();
    const seen = await recorded(page);
    for (const kind of ["space", "subview", "section"]) expect(seen).toContain(kind);
  });

  test("the lens and the Home item are the same element to the transition", async ({ page, theme }) => {
    await openApp(page, { theme });
    await page.evaluate(() => { document.documentElement.dataset.routeMotion = "space"; });
    await expect(page.locator(".landing .amara-lens")).toHaveCSS("view-transition-name", "amara-lens");
    await page.evaluate(() => { delete document.documentElement.dataset.routeMotion; });
    await expect(page.locator(".landing .amara-lens")).toHaveCSS("view-transition-name", "none");
    await navButton(page, "Settings").click();
    await page.evaluate(() => { document.documentElement.dataset.routeMotion = "space"; });
    await expect(page.locator("[data-system=terrance] > i")).toHaveCSS("view-transition-name", "amara-lens");
  });

  test("a loaded screen opens with no skeleton", async ({ page, theme }) => {
    const lastChunk = page.waitForResponse(/ListsScreen-[^/]+\.js/); // the end of the background queue
    await openApp(page, { theme });
    await lastChunk;
    await record(page);
    for (const label of ["Agents", "Mind–Body", "Business", "Settings", "Smart home"]) {
      await navButton(page, label).click();
      await expect(activeField(page, label).getByRole("heading", { level: 1 }).first()).toBeVisible();
    }
    expect(await recorded(page)).not.toContain("skeleton");
  });
});

test.describe("interruption and input", () => {
  test("the new page is live while it enters", async ({ page, theme }) => {
    await openApp(page, { theme });
    await navButton(page, "Business").click();
    await activeField(page, "Business").getByRole("heading", { level: 1 }).waitFor();
    await expect.poll(() => motion(page)).toBeNull();
    const hit = await page.evaluate(async () => {
      document.querySelector<HTMLElement>("[data-system=settings]")!.click();
      while (!document.documentElement.dataset.routeMotion || !document.querySelector(".settings")) await new Promise((next) => requestAnimationFrame(next));
      await new Promise((next) => requestAnimationFrame(next));
      const box = document.querySelector(".settings .page-hero__title")!.getBoundingClientRect();
      return { during: !!document.documentElement.dataset.routeMotion, inPage: !!document.elementFromPoint(box.x + 10, box.y + 10)?.closest(".active-field") };
    });
    expect(hit).toEqual({ during: true, inPage: true });
  });

  test("clicks land during a transition, and the last click wins", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme });
    await navButton(page, "Business").click();
    await expect(activeField(page, "Business").getByRole("heading", { level: 1 })).toHaveText("Business");
    await expect.poll(() => motion(page)).toBeNull();
    const target = async (label: string) => { const box = (await navButton(page, label).boundingBox())!; return [box.x + box.width / 2, box.y + box.height / 2] as const; };
    const [settingsX, settingsY] = await target("Settings");
    const [chatX, chatY] = await target("Agents");
    // 30 ms apart: well inside each transition, and faster than a hand. WebKit can miss a click sent
    // in the same frame the capture starts, which no person can do.
    await page.mouse.click(settingsX, settingsY);
    await page.waitForTimeout(30);
    await page.mouse.click(chatX, chatY);
    await page.waitForTimeout(30);
    await page.mouse.click(settingsX, settingsY);
    await expect(activeField(page, "Settings").getByRole("heading", { level: 1 })).toHaveText("Settings");
    await expect.poll(() => motion(page)).toBeNull();
    expect(consoleErrors).toEqual([]);
  });
});

test.describe("focus and scroll", () => {
  test("focus lands on the new page's heading, for the sidebar, Home and ⌘K", async ({ page, theme, browserName }) => {
    await openApp(page, { theme });
    // The app clears data-route-motion when the transition and the enter animation are both done: wait on that, not a timeout.
    await navButton(page, "Settings").click();
    await expect(activeField(page, "Settings").getByRole("heading", { level: 1 })).toBeFocused();
    await expect.poll(() => motion(page)).toBeNull();
    await navButton(page, "Home").click();
    await expect(page.locator(".landing h1")).toBeFocused();
    await expect.poll(() => motion(page)).toBeNull();
    if (browserName !== "webkit") { // WebKit's Tab skips buttons unless the Mac's full keyboard access is on
      await page.keyboard.press("Tab");
      // design-1110b A: the Home chat under the lens is the first stop, before the buttons.
      await expect(page.locator(".landing .hchat button").first()).toBeFocused();
    }
    await page.keyboard.press("Meta+k");
    await page.getByRole("combobox", { name: "Search or ask Amara" }).fill("rev");
    await page.keyboard.press("Enter");
    await expect(activeField(page, "Business").getByRole("heading", { level: 1 })).toBeFocused();
  });

  test("arrow keys stay on the segmented control while its sub-views change", async ({ page, theme }) => {
    await openApp(page, { theme });
    const field = await openView(page, "Fitness");
    await field.getByRole("radio", { name: "Fitness", exact: true }).focus();
    await page.keyboard.press("ArrowRight");
    await expect(field.getByRole("heading", { level: 1 })).toHaveText("Neuro");
    await expect(field.getByRole("radio", { name: "Neuro", exact: true })).toBeFocused();
    await page.keyboard.press("ArrowRight");
    await expect(field.getByRole("heading", { level: 1 })).toHaveText("Neuro history");
    await expect(field.getByRole("radio", { name: "Neuro history", exact: true })).toBeFocused();
  });

  test("a new screen starts at the top", async ({ page, theme }) => {
    await page.setViewportSize({ width: 1000, height: 620 });
    await openApp(page, { theme });
    await navButton(page, "Settings").click();
    await activeField(page, "Settings").getByRole("heading", { level: 1 }).waitFor();
    await openMore(page);
    const pane = page.locator(".active-field");
    await pane.evaluate((el) => { el.scrollTop = 400; });
    expect(await pane.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
    await navButton(page, "Business").click();
    await activeField(page, "Business").getByRole("heading", { level: 1 }).waitFor();
    await expect.poll(() => pane.evaluate((el) => el.scrollTop)).toBe(0);
  });
});

test.describe("fallbacks", () => {
  test("reduced motion: no transition runs, and focus still moves", async ({ page, theme }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openApp(page, { theme });
    await record(page);
    await navButton(page, "Settings").click();
    await expect(activeField(page, "Settings").getByRole("heading", { level: 1 })).toBeFocused();
    await navButton(page, "Home").click();
    await expect(page.locator(".landing h1")).toBeFocused();
    expect((await recorded(page)).filter((kind) => kind !== "skeleton")).toEqual([]);
  });

  test("a WebKit without View Transitions fades the new view in and still navigates", async ({ page, theme, consoleErrors }) => {
    await page.addInitScript(() => { delete (Document.prototype as Partial<Document>).startViewTransition; });
    await openApp(page, { theme });
    await navButton(page, "Settings").click();
    await expect(page.locator(".active-field").getByRole("heading", { level: 1 })).toHaveText("Settings");
    await expect.poll(() => motion(page)).toBeNull();
    await navButton(page, "Business").click();
    await expect(page.locator(".active-field").getByRole("heading", { level: 1 })).toHaveText("Business");
    expect(consoleErrors).toEqual([]);
  });
});
