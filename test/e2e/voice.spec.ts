import { activeField, expect, navButton, openApp, test, openMore } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** Chat & Agents › Voice (commands/voice.rs). VOICE_SHOTS=<dir> also saves screenshots there. */
const shot = async (page: import("@playwright/test").Page, name: string) => {
  if (process.env.VOICE_SHOTS) await page.screenshot({ path: `${process.env.VOICE_SHOTS}/${name}.png`, fullPage: true });
};

test("Voice: live state, voice picker and recent turns", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await field.getByRole("radio", { name: "Voice" }).click();
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Voice");
  const card = field;
  await expect(card.getByLabel("Amara Voice: Listening")).toBeVisible();
  await expect(card.getByRole("button", { name: "Stop" })).toBeVisible();
  await openMore(page);
  await expect(field.getByLabel("Voice", { exact: true })).toHaveValue("neosoul_g");
  const rows = field.getByRole("list", { name: "Recent turns" }).getByRole("listitem");
  await expect(rows).toHaveCount(3);
  await expect(rows.first()).toContainText("first audio 5.4 s");
  await expect(rows.nth(1)).toContainText("Ignored");
  await shot(page, `voice-${theme}`);
  expect(consoleErrors).toEqual([]);
});

test("Voice: not installed", async ({ page, theme, consoleErrors }) => {
  const data = { ...responses(), voice_status: { installed: false, running: false, pid: null, state: "not_installed", state_age_s: null, voice: "neosoul_g", rss_mb: null, launchd: false, dir: "/Users/taisun/projects/amara-voice" } };
  await openApp(page, { theme, data });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await field.getByRole("radio", { name: "Voice" }).click();
  await expect(field.getByText("Amara Voice isn’t set up on this Mac")).toBeVisible();
  await expect(field.getByRole("button", { name: "Start" })).toHaveCount(0);
  await shot(page, `voice-not-installed-${theme}`);
  expect(consoleErrors).toEqual([]);
});
