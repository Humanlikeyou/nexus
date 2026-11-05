import fs from "node:fs";
import { activeField, expect, navButton, openApp, test, openMore } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

const SHOTS = process.env.MIND_BODY_SHOTS; // e.g. /tmp/mind-body-shots (screenshots only when set)
const shot = async (page: import("@playwright/test").Page, name: string) => {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });
};

test.describe("Mind–Body", () => {
  test("data state: three panels, readiness, n and disclaimer", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme });
    await navButton(page, "Mind–Body").click();
    const f = activeField(page, "Mind–Body");
    await expect(f.getByRole("heading", { level: 1 })).toHaveText("Mind–Body");
    await openMore(page);
    for (const name of ["Readiness", "Focus drivers", "Session body context"]) await expect(f.getByRole("heading", { level: 2, name })).toBeVisible();
    await expect(f.getByLabel(/^Readiness: (High|Moderate|Low)$/)).toBeVisible();
    await expect(f.getByText("Collecting data: 1/15 sessions")).toBeVisible();
    await expect(f.getByText("n = 1 matched sessions")).toBeVisible();
    await expect(f.getByText("Correlation, not causation.")).toBeVisible();
    await expect(f.getByText("Personal estimates, not medical data.").last()).toBeVisible();
    await expect(f.getByRole("img", { name: /^HRV, last \d+ days/ })).toBeVisible();
    expect(consoleErrors).toEqual([]);
    await shot(page, `data-${theme}`);
  });

  test("binary_missing: setup steps in .subsystem-inactive", async ({ page, theme }) => {
    const data = { ...responses(), noop_query: { __reject: { kind: "binary_missing", detail: "NOOP isn't installed. Build it with swift build -c release, then set its path in Settings." } } };
    await openApp(page, { theme, data });
    await navButton(page, "Mind–Body").click();
    const f = activeField(page, "Mind–Body");
    const inactive = f.locator(".subsystem-inactive");
    await expect(inactive).toBeVisible();
    await expect(inactive).toContainText("NOOP isn't installed.");
    await expect(f.getByRole("button", { name: "Check again" })).toBeVisible();
    await shot(page, `inactive-${theme}`);
  });

  test("bad_output: retry state, no crash", async ({ page, theme }) => {
    const data = { ...responses(), noop_query: { __reject: { kind: "bad_output", detail: "x" } } };
    await openApp(page, { theme, data });
    await navButton(page, "Mind–Body").click();
    const f = activeField(page, "Mind–Body");
    await expect(f.getByText("Couldn't load this chart.")).toBeVisible();
    await expect(f.getByRole("button", { name: "Retry" })).toBeVisible();
  });

  test("Settings › WHOOP controls", async ({ page, theme }) => {
    await openApp(page, { theme });
    await navButton(page, "Settings").click();
    const f = activeField(page, "Settings");
    await openMore(page);
    await expect(f.getByRole("radio", { name: "NOOP (recommended)" })).toHaveAttribute("aria-checked", "true");
    await f.getByLabel("NOOP binary path").fill("relative/noop");
    await expect(f.getByText("Use a full path that starts with /.")).toBeVisible();
    await f.locator("#noop-settings").scrollIntoViewIfNeeded();
    if (SHOTS) await f.locator("#noop-settings").screenshot({ path: `${SHOTS}/settings-${theme}.png` });
  });
});
