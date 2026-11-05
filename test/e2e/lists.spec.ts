import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, openApp, openView, setMock, test, type Theme, openMore } from "./fixtures/tauri-mock";
import { FOLLOWUPS, responses, type Responses } from "./fixtures/snapshot";

const open = async (page: Page, theme: Theme, data: Responses = responses()) => {
  await openApp(page, { theme, data });
  const field = await openView(page, "Lists");
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Lists");
  return field;
};
const callsOf = (page: Page, cmd: string) => page.evaluate((name) => (window as unknown as { __E2E_CALLS: { cmd: string; args?: unknown }[] }).__E2E_CALLS.filter((c) => c.cmd === name).map((c) => c.args), cmd);
const refocus = (page: Page) => page.evaluate(() => { window.dispatchEvent(new Event("focus")); });

test.describe("Chat & Agents › Lists", () => {
  test("shows the three sections; the other sub-views stay in place", async ({ page, theme, consoleErrors }) => {
    const field = await open(page, theme);
    await expect(field.getByRole("radio")).toHaveText(["Chat", "Activity", "Lists", "Crews", "Voice"]);
    await openMore(page);
    for (const name of ["Open", "Done recently", "Not mentioning"]) await expect(field.getByRole("heading", { level: 2, name })).toBeVisible();
    await expect(field.getByRole("list", { name: "Open items" }).getByRole("listitem")).toHaveCount(4);
    await expect(field.getByText("Overdue")).toBeVisible();
    await expect(field.getByText("Due today")).toBeVisible();
    await expect(field.getByText("Ended Oct 20. Amara can bring this up again.")).toBeVisible();
    expect(consoleErrors).toEqual([]);
  });

  test("⌘K reaches Lists", async ({ page, theme }) => {
    await openApp(page, { theme });
    await page.keyboard.press("Meta+k");
    await page.getByRole("combobox", { name: "Search or ask Amara" }).fill("Lists");
    await page.keyboard.press("Enter");
    await expect(page.getByRole("radio", { name: "Lists" })).toHaveAttribute("aria-checked", "true");
  });

  test("empty states", async ({ page, theme }) => {
    const field = await open(page, theme, { ...responses(), followups_list: { open: [], recentlyDone: [], muted: [] } });
    await expect(field.getByText("Nothing open. Amara isn't following up on anything.")).toBeVisible();
    await openMore(page);
    await expect(field.getByText("Nothing done in the last 30 days.")).toBeVisible();
    await expect(field.getByText("No muted topics. Amara can bring up anything.")).toBeVisible();
  });

  test("delete confirms first: keyboard Cancel keeps it, Delete removes it", async ({ page, theme, consoleErrors }) => {
    const field = await open(page, theme);
    const remove = field.getByRole("button", { name: "Delete: Send the release notes draft" });
    await remove.click();
    const dialog = page.getByRole("dialog", { name: "Delete this item?" });
    await expect(dialog).toContainText("“Send the release notes draft” will be gone");
    await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(remove).toBeFocused();
    expect(await callsOf(page, "followups_delete_item")).toEqual([]);
    await remove.click();
    await setMock(page, "followups_list", { ...FOLLOWUPS, open: FOLLOWUPS.open.slice(1) });
    await dialog.getByRole("button", { name: "Delete" }).click();
    await expect(remove).toHaveCount(0);
    expect(await callsOf(page, "followups_delete_item")).toEqual([{ id: 1 }]);
    await expect(field.locator(".lists__section-title", { hasText: "Open" })).toBeFocused();
    expect(consoleErrors).toEqual([]);
  });

  test("an edit in progress survives a refresh, and saves only what changed", async ({ page, theme }) => {
    const changed = { ...FOLLOWUPS, open: [{ ...FOLLOWUPS.open[0], next: "Amara's new step" }, ...FOLLOWUPS.open.slice(1), { ...FOLLOWUPS.open[3], id: 40, text: "Added by Amara" }] };
    const field = await open(page, theme);
    await field.getByRole("button", { name: "Edit: Send the release notes draft" }).click();
    const text = field.getByLabel("Text");
    await expect(text).toBeFocused();
    await text.fill("Send the final release notes");
    await setMock(page, "followups_list", changed);
    await refocus(page);
    await expect(text).toHaveValue("Send the final release notes");
    await expect(text).toBeFocused();
    await openMore(page); // a fifth open item waits behind the disclosure
    await expect(field.getByText("Added by Amara")).toBeVisible();
    await expect(field.getByText(/Amara updated this while you were editing/)).toBeVisible();
    await field.getByRole("button", { name: "Save", exact: true }).click();
    await expect(text).toHaveCount(0);
    expect(await callsOf(page, "followups_update_item")).toEqual([{ id: 1, patch: { text: "Send the final release notes" } }]);
    await expect(field.getByRole("button", { name: "Edit: Send the release notes draft" })).toBeFocused();
  });

  test("shows the text the backend stored, not what was typed", async ({ page, theme }) => {
    const cleaned = { ...FOLLOWUPS.open[0], text: "Send the notes" };
    const field = await open(page, theme, { ...responses(), followups_update_item: cleaned });
    await field.getByRole("button", { name: "Edit: Send the release notes draft" }).click();
    await field.getByLabel("Text").fill("Send the notes\u200B and more");
    await setMock(page, "followups_list", { ...FOLLOWUPS, open: [cleaned, ...FOLLOWUPS.open.slice(1)] });
    await field.getByRole("button", { name: "Save", exact: true }).click();
    await expect(field.getByRole("button", { name: "Edit: Send the notes", exact: true })).toBeVisible();
    await expect(field.getByText(/cleaned up for safety/)).toBeVisible();
  });

  test("an item removed while being edited keeps the edit", async ({ page, theme }) => {
    const field = await open(page, theme);
    await field.getByRole("button", { name: "Edit: Send the release notes draft" }).click();
    await field.getByLabel("Text").fill("Still wanted");
    await setMock(page, "followups_list", { ...FOLLOWUPS, open: FOLLOWUPS.open.slice(1) });
    await refocus(page);
    await expect(field.getByText(/no longer on the list/)).toBeVisible();
    await expect(field.getByLabel("Text")).toHaveValue("Still wanted");
    await field.getByRole("button", { name: "Save as new item" }).click();
    await expect(field.getByLabel("Text")).toHaveCount(0);
    expect(await callsOf(page, "followups_add_item")).toEqual([{ text: "Still wanted", due: "2026-10-25", next: "Ask for a second read first" }]);
  });

  test("malformed file: plain message, no code, no empty state, Try again recovers", async ({ page, theme, consoleErrors }) => {
    const field = await open(page, theme, { ...responses(), followups_list: { __reject: "malformed_file" } });
    await expect(field.getByRole("alert")).toContainText("Amara's list couldn't be read, so nothing was changed.");
    await expect(field).not.toContainText("malformed");
    await expect(field.getByText(/Nothing open/)).toHaveCount(0);
    await setMock(page, "followups_list", FOLLOWUPS);
    await field.getByRole("button", { name: "Try again" }).click();
    await expect(field.getByRole("list", { name: "Open items" })).toBeVisible();
    await expect(field.getByRole("alert")).toHaveCount(0);
    expect(consoleErrors).toEqual([]);
  });

  test("a build without the commands says so plainly", async ({ page, theme }) => {
    const data = responses();
    delete data.followups_list;
    const field = await open(page, theme, data);
    await expect(field.getByRole("alert")).toContainText("Lists aren't in this version of Amara OS yet.");
  });

  test("Done, Reopen, add item and mute send the right commands", async ({ page, theme }) => {
    const field = await open(page, theme);
    await field.getByRole("button", { name: "Done: Pick a date for the sample dinner" }).click();
    await openMore(page);
    await field.getByRole("button", { name: "Reopen: Return the sample package" }).click();
    await field.getByRole("button", { name: "Add item", exact: true }).click();
    const add = field.getByRole("form", { name: "Add item" });
    await add.getByLabel("Text").fill("Buy stamps");
    await add.getByLabel("Due date (optional)").fill("2026-11-01");
    await page.keyboard.press("Enter");
    await expect(add).toHaveCount(0);
    await expect(field.getByRole("button", { name: "Add item", exact: true })).toBeFocused();
    const topic = field.getByRole("form", { name: "Add topic" });
    await topic.getByLabel("Topic").fill("taxes");
    await topic.getByLabel("Until (optional)").fill("2026-12-01");
    await topic.getByRole("button", { name: "Add topic" }).click();
    await expect(topic.getByLabel("Topic")).toHaveValue("");
    await field.getByRole("button", { name: "Remove: gym schedule" }).click();
    await page.getByRole("dialog", { name: "Remove this topic?" }).getByRole("button", { name: "Remove" }).click();
    await expect.poll(() => callsOf(page, "followups_delete_muted_topic")).toEqual([{ topic: "gym schedule" }]);
    expect(await callsOf(page, "followups_mark_done")).toEqual([{ id: 4 }]);
    expect(await callsOf(page, "followups_reopen_item")).toEqual([{ id: 5 }]);
    expect(await callsOf(page, "followups_add_item")).toEqual([{ text: "Buy stamps", due: "2026-11-01" }]);
    expect(await callsOf(page, "followups_add_muted_topic")).toEqual([{ topic: "taxes", until: "2026-12-01" }]);
  });

  test("a keyboard-only edit: Tab to Edit, Enter, type, Enter saves", async ({ page, theme }) => {
    const field = await open(page, theme);
    const edit = field.getByRole("button", { name: "Edit: Renew the library card" });
    await edit.focus();
    await page.keyboard.press("Enter");
    await page.keyboard.type(" soon");
    await page.keyboard.press("Enter");
    await expect.poll(() => callsOf(page, "followups_update_item")).toEqual([{ id: 3, patch: { text: "Renew the library card soon" } }]);
    await expect(edit).toBeFocused();
    const ring = await edit.evaluate((el) => getComputedStyle(el).outlineColor);
    expect(ring).toBe(await page.evaluate(() => { const probe = document.createElement("i"); probe.style.color = "var(--focus-ring)"; document.body.append(probe); const color = getComputedStyle(probe).color; probe.remove(); return color; }));
  });

  test("no serious or critical axe violations: list, editor and dialog", async ({ page, theme }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    const field = await open(page, theme);
    await field.getByRole("list", { name: "Open items" }).waitFor();
    await page.waitForTimeout(400);
    const bad = async () => (await new AxeBuilder({ page }).analyze()).violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes[0]?.target}`);
    expect(await bad()).toEqual([]);
    await field.getByRole("button", { name: "Edit: Renew the library card" }).click();
    await field.getByRole("button", { name: "Add item", exact: true }).click();
    expect(await bad()).toEqual([]);
    await field.getByRole("button", { name: "Delete: Send the release notes draft" }).click();
    await page.waitForTimeout(300);
    expect(await bad()).toEqual([]);
  });

  test("the page fits a narrow window with no sideways scroll", async ({ page, theme }) => {
    const field = await open(page, theme);
    await page.setViewportSize({ width: 900, height: 620 });
    await field.getByRole("button", { name: "Edit: Renew the library card" }).click();
    expect(await field.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  });

  test.describe("sign-off shot", () => {
    test.skip(() => !process.env.LISTS_SHOTS, "LISTS_SHOTS not set");
    test("1280x800", async ({ page, theme }) => {
      await page.setViewportSize({ width: 1280, height: 800 });
      const field = await open(page, theme);
      await field.getByRole("list", { name: "Open items" }).waitFor();
      await page.waitForTimeout(500);
      await page.screenshot({ path: `${process.env.LISTS_SHOTS}/lists-1280x800-${theme}.png` });
    });
  });
});
