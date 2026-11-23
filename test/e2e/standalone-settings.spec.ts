import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, openSettingsTab, routeSettled, test } from "./fixtures/tauri-mock";

/** build-1111s: standalone-app screens. Settings tabs, Services, Devices (WHOOP 4), Notifications, Backup, Updates, first-run. */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const FORBIDDEN = /NOOP|OpenClaw|separate program/;
const preview = (page: Page, opts: Record<string, unknown> = {}) => page.addInitScript((o) => {
  localStorage.setItem("nexus.sampleAdapters", "1");
  localStorage.setItem("nexus.standalonePreview", JSON.stringify(o));
}, opts);

const TABS = ["General", "Services", "Devices", "Notifications", "Backup"];
for (const sample of [false, true]) {
  for (const tab of TABS) {
    test(`(1111s) Settings › ${tab}${sample ? " with sample adapters" : ""}: axe clean (incl. 4.5:1), no NOOP/OpenClaw framing, honest tags`, async ({ page, theme }) => {
      if (sample) await preview(page);
      await openApp(page, { theme, reduceMotion: true });
      const field = await openSettingsTab(page, tab);
      await expect(field.getByRole("heading", { level: 1 })).toHaveText(tab === "General" ? "Settings" : tab);
      await page.waitForTimeout(300);
      expect(await blocking(page)).toEqual([]);
      expect(await field.textContent()).not.toMatch(FORBIDDEN);
      if (!sample) await expect(field.getByText("Sample data")).toHaveCount(0);
      else await expect(field.getByText("Sample data").first()).toBeVisible();
    });
  }
}

test("(1111s) the Settings tab bar is the Business bar, in order, and old anchors land on their tab", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true });
  await navButton(page, "Settings").click();
  const bar = page.locator(".subview-bar[data-host=settings]");
  await expect(bar.getByRole("radio")).toHaveText(["General", "Services", "Devices", "Notifications", "Backup", "Voice", "Memory"]);
  await expect(bar.getByRole("radio", { name: "General" })).toHaveAttribute("aria-checked", "true");
  await navButton(page, "Smart home").click();
  const open = activeField(page, "Smart home").getByRole("button", { name: "Open Home Assistant settings" });
  if (await open.count()) {
    await open.click();
    await expect(bar.getByRole("radio", { name: "Devices" })).toHaveAttribute("aria-checked", "true");
    await expect(activeField(page, "Settings").locator("#home-assistant")).toBeVisible();
  }
});

test("(1111s) sidebar summary opens Services; services show every state in plain words", async ({ page, theme }) => {
  await preview(page);
  await openApp(page, { theme, reduceMotion: true });
  await navButton(page, "Enterprise").click();
  await page.getByRole("button", { name: /1 service needs attention/ }).click();
  const field = activeField(page, "Settings");
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Services");
  await expect(field.getByText("1 needs attention")).toBeVisible();
  await expect(field.getByRole("button", { name: "Restart Crews" })).toBeDisabled();
  await field.getByRole("button", { name: "Logs for Automations (n8n)" }).click();
  const dialog = page.getByRole("dialog", { name: "Automations (n8n) · what happened" });
  await expect(dialog).toBeVisible();
  expect(await blocking(page)).toEqual([]);
  await dialog.getByRole("button", { name: "Close" }).click();
  await expect(dialog).toBeHidden();
});

test("(1111s) Notifications: no quiet hours or rate limit; the token is write-only and never shown", async ({ page, theme }) => {
  await preview(page, { relayKeys: false });
  await openApp(page, { theme, reduceMotion: true });
  const field = await openSettingsTab(page, "Notifications");
  await expect(field).not.toContainText(/Quiet hours|At most|30 min/);
  await expect(field.getByRole("switch", { name: "Telegram relay" })).toBeDisabled();
  await field.getByRole("button", { name: "Add…" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Telegram bot token" });
  await expect(dialog).toBeVisible();
  const secret = "e2e-SENTINEL-telegram-token-123";
  await dialog.getByLabel("Telegram bot token").fill(secret);
  expect(await blocking(page)).toEqual([]);
  await dialog.getByRole("button", { name: "Save to Keychain" }).click();
  await expect(dialog).toBeHidden();
  expect(await page.content()).not.toContain(secret);
  await expect(field.getByRole("status").filter({ hasText: "Saved to the Keychain." })).toBeVisible();
  await expect(field.getByRole("button", { name: "Change…" })).toBeVisible(); // token row now reads Set
});

test("(1111s) Devices: the takeover notice shows only during the transition", async ({ page, theme }) => {
  await preview(page, { whoopOwner: "legacy" });
  await openApp(page, { theme, reduceMotion: true });
  const field = await openSettingsTab(page, "Devices");
  await expect(field.getByRole("heading", { name: "Another app is using your WHOOP; Amara will take over" })).toBeVisible();
  await expect(field.getByRole("button", { name: "Sync Now" })).toBeDisabled();
  expect(await blocking(page)).toEqual([]);
  expect(await field.textContent()).not.toMatch(FORBIDDEN);
});

test("(1111s) Backup: restore asks first and saves today; first-run opens from General and walks its steps", async ({ page, theme }) => {
  await preview(page);
  await openApp(page, { theme, reduceMotion: true });
  const field = await openSettingsTab(page, "Backup");
  await field.getByRole("button", { name: "Restore from a Backup…" }).click();
  // Asks first: the dialog opens before anything is replaced, and nothing has been restored yet.
  const dialog = page.getByRole("dialog", { name: "Restore from a backup?" });
  await expect(dialog.getByText("This replaces current Amara OS config and local data with the archive's contents. This can't be undone.")).toBeVisible();
  await expect(page.getByText(/Restored from the backup of/)).toHaveCount(0);
  const confirm = dialog.getByRole("button", { name: "Save Today, Then Restore" });
  await expect(confirm).toBeDisabled(); // nothing chosen yet, so nothing can be confirmed
  // Pick a backup: the real preview (check, date, size, contents) and the safety-copy wording appear.
  await dialog.getByRole("group", { name: "Backups" }).getByRole("button", { name: "Oct 21, 3:00 AM" }).click();
  const picked = page.getByRole("dialog", { name: "Restore from amara-backup-oct-21?" });
  await expect(picked.getByText("Backup check")).toBeVisible();
  await expect(picked.getByText("Passed")).toBeVisible();
  await expect(picked.getByText("8.1 GB")).toBeVisible();
  await expect(picked.getByText("Chats and Today · 1,120 messages")).toBeVisible();
  await expect(picked.getByText(/Your current data is saved to a safety backup in the Amara OS data folder before anything is replaced\./)).toBeVisible();
  await expect(picked.getByText(/confirm once more in a native dialog/)).toBeVisible();
  expect(await blocking(page)).toEqual([]);
  // Cancel changes nothing; reopen, pick, and confirm to reach the result line.
  await picked.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByText(/Restored from the backup of/)).toHaveCount(0);
  await field.getByRole("button", { name: "Restore from a Backup…" }).click();
  await page.getByRole("dialog", { name: "Restore from a backup?" }).getByRole("button", { name: "Oct 21, 3:00 AM" }).click();
  await expect(page.getByRole("button", { name: "Save Today, Then Restore" })).toBeEnabled();
  await page.getByRole("button", { name: "Save Today, Then Restore" }).click();
  await expect(page.getByText("Restored from the backup of Oct 21, 3:00 AM. Restart Amara OS to load the restored data.")).toBeVisible();
  await expect(page.getByText("Safety backup of current data saved (sample)")).toBeVisible();
  const general = await openSettingsTab(page, "General");
  await general.getByRole("button", { name: "Run Setup Again…" }).click();
  const setup = page.getByRole("dialog", { name: "Set up Amara OS" });
  await expect(setup.getByRole("heading", { level: 1 })).toHaveText("Welcome to Amara");
  expect(await blocking(page)).toEqual([]);
  await setup.getByRole("button", { name: "Continue" }).click();
  await expect(setup.getByText("n8n encryption key")).toBeVisible();
  await setup.getByRole("button", { name: "Restore", exact: true }).click();
  await expect(setup.getByRole("radio", { name: /Large/ })).toBeDisabled();
  expect(await blocking(page)).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(setup).toBeHidden();
  await routeSettled(page).catch(() => undefined);
});
