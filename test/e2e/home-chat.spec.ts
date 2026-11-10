import { expect, openApp, openHomeChat, navButton, emitTauriEvent, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** design-1110b A: chat on Home, under the lens. Collapsed = last 3 messages; Expand is the largest; ⌘1 focuses it. */
test("Home chat: last three messages, voice turns in the same thread, Expand and ⌘1", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme });
  const chat = await openHomeChat(page);
  const log = chat.getByRole("list", { name: "Messages" });
  await expect(log.locator("li.chat-message")).toHaveCount(3);
  // Spoken turns from voice_recent_turns are tagged, typed ones aren't.
  const all = responses() as Record<string, unknown>;
  const spoken = (all.voice_recent_turns as { route?: string; text?: string }[]).filter((t) => t.route !== "ignored" && t.text);
  if (spoken.length > 0) {
    await chat.getByRole("button", { name: "Expand" }).click();
    await expect(chat.getByRole("button", { name: "Collapse" })).toHaveAttribute("aria-expanded", "true");
    await expect(page.locator(".landing")).toHaveAttribute("data-chat", "expanded");
    await expect(log.locator('li.chat-message[data-voice]').first()).toBeVisible();
    expect(await log.locator("li.chat-message").count()).toBeGreaterThan(3);
    await expect(chat.getByRole("button", { name: /Full page/ })).toHaveCount(0);
    await chat.getByRole("button", { name: "Collapse" }).click();
    await expect(page.locator(".landing")).toHaveAttribute("data-chat", "collapsed");
  }
  // ⌘1 from another section comes back Home with the composer focused.
  await navButton(page, "Agents").click();
  await page.keyboard.press("Meta+1");
  await expect(page.locator(".landing .hchat #chat-composer")).toBeFocused();
  expect(consoleErrors).toEqual([]);
});

test("Home chat: while Amara speaks, her newest reply is marked Speaking", async ({ page, theme }) => {
  await openApp(page, { theme });
  const chat = await openHomeChat(page);
  await emitTauriEvent(page, "amara-voice://state", { ts: Date.now(), state: "speaking" });
  await expect(chat.locator('li.chat-message[data-speaking]')).toHaveCount(1);
  await expect(chat.locator('li.chat-message[data-speaking]')).toHaveAttribute("data-role", "assistant");
  await expect(chat.getByText("Speaking", { exact: true })).toBeVisible();
});
