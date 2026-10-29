import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** Settings › Backup › Restore: picker → destructive Dialog → unpack_backup → result list. */
test("restore from backup asks first, then shows what was restored", async ({ page, theme, consoleErrors }) => {
  await openApp(page, {
    theme,
    data: {
      ...responses(),
      "plugin:dialog|open": "/Users/me/Desktop/nexus-backup-20261029_101500.tar.gz",
      unpack_backup: ["Safety backup of current data saved to /data/restore-backups/20261029_101600-full", "Amara OS data: 4 files restored"],
    },
  });
  await navButton(page, "Settings").click();
  const field = activeField(page, "Settings");
  await field.getByRole("button", { name: "Choose…" }).click();
  await expect(field.getByLabel("Backup file")).toHaveValue("/Users/me/Desktop/nexus-backup-20261029_101500.tar.gz");

  await field.getByRole("button", { name: "Restore from Backup…" }).click();
  const dialog = page.getByRole("dialog", { name: "Restore from backup?" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("This replaces current Amara OS config and local data with the archive's contents. This can't be undone.")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
  await dialog.getByRole("button", { name: "Restore", exact: true }).click();

  await expect(field.getByText("Amara OS data: 4 files restored")).toBeVisible();
  await expect(field.getByText(/Safety backup of current data saved to/)).toBeVisible();
  await expect(dialog).toBeHidden();
  expect(consoleErrors).toEqual([]);
});
