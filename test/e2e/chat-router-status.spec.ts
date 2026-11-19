import { activeField, expect, navButton, openApp, test, openHomeChat } from "./fixtures/tauri-mock";
import { NOW, responses } from "./fixtures/snapshot";

/** QA 11-04 retest2: router 503 model loading, router-reported tier/model, Home/End. */
const LOADING = { __reject: { kind: "model_loading", retryAfter: 1 } };
const reply = (over: Record<string, unknown> = {}) => ({
  id: "r1", conversationId: "c", hermesSessionId: "api-e2e", runtime: "hermes", model: "gemma4-12b-uncensored", provider: "custom", status: "succeeded",
  userMessage: "/deep plan my week", assistantMessage: "Here's your week.", error: null, startedAt: NOW, completedAt: NOW, durationMs: 4200,
  contextItemIds: [], actionId: null, runtimeProfile: "nexus-managed-actions-v5", tier: null, ...over,
});
const data = (interact: unknown) => {
  const base = responses();
  return {
    ...base,
    get_terrence_runtime_health: { ...(base.get_terrence_runtime_health as object), model: "gemma4-12b-uncensored", routedViaRouter: true },
    list_terrence_interactions: [],
    prepare_amara_turn: { sections: [], itemIds: [] },
    interact_with_terrence: interact,
  };
};
async function send(page: import("@playwright/test").Page, text: string) {
  const field = await openHomeChat(page);
  await field.getByLabel("Message Amara").fill(text);
  await field.getByLabel("Message Amara").press("Enter");
  return field;
}

test("503 model loading: shows loading, retries, then answers", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, data: data({ __sequence: [LOADING, reply({ assistantMessage: "Loaded and ready." })] }) });
  const field = await send(page, "hello");
  await expect(field.getByRole("status", { name: "Amara is loading her model…" })).toBeVisible();
  await page.clock.fastForward(1500);
  await expect(field.getByText("Loaded and ready.")).toBeVisible();
  await expect(field.getByText("Amara is loading her model…")).toHaveCount(0);
  expect(consoleErrors).toEqual([]);
});

test("503 model loading: after 3 retries an error with Retry; Cancel works during the wait", async ({ page, theme }) => {
  await openApp(page, { theme, data: data(LOADING) });
  const field = await send(page, "hello");
  for (let i = 0; i < 3; i++) {
    await expect(field.getByRole("status", { name: "Amara is loading her model…" })).toBeVisible();
    await page.clock.fastForward(1500);
  }
  await expect(field.getByText("Amara's model is still loading. Try again in a moment.")).toBeVisible();
  await expect(field.getByRole("button", { name: "Retry" })).toBeVisible();
  const calls = await page.evaluate(() => (window as unknown as { __E2E_CALLS: Array<{ cmd: string }> }).__E2E_CALLS.filter((c) => c.cmd === "interact_with_terrence").length);
  expect(calls).toBe(4);

  await field.getByLabel("Message Amara").fill("again");
  await field.getByLabel("Message Amara").press("Enter");
  await expect(field.getByRole("status", { name: "Amara is loading her model…" })).toBeVisible();
  await field.getByRole("button", { name: "Cancel waiting for Amara's reply" }).click();
  await expect(field.getByRole("status", { name: "Amara is loading her model…" })).toHaveCount(0);
  await page.clock.fastForward(5000);
  const after = await page.evaluate(() => (window as unknown as { __E2E_CALLS: Array<{ cmd: string }> }).__E2E_CALLS.filter((c) => c.cmd === "interact_with_terrence").length);
  expect(after).toBe(5);
});

test("tier badge and header model come from the router response", async ({ page, theme }) => {
  await openApp(page, { theme, data: data(reply({ route: { tier: "deep", model: "qwen3-32b", reason: "slash_deep" } })) });
  const field = await send(page, "/deep plan my week");
  const list = field.getByRole("list", { name: "Messages" });
  await expect(list.getByLabel("Answered on the Deep tier")).toHaveText("Deep");
  await expect(field.locator(".chat-screen__model")).toHaveText("qwen3-32b");
});

test("without router fields: no badge, header doesn't claim Hermes's default model", async ({ page, theme }) => {
  await openApp(page, { theme, data: data(reply()) });
  const field = await send(page, "/deep plan my week");
  await expect(field.getByText("Here's your week.")).toBeVisible();
  await expect(field.getByRole("list", { name: "Messages" }).getByText(/^(Fast|Deep)$/)).toHaveCount(0);
  await expect(field.locator(".chat-screen__model")).toHaveText("Amara Router");
});

test("Home scrolls to the top, End to the bottom, from an empty composer, not while typing", async ({ page, theme }) => {
  const long = "Line of the plan.\n".repeat(80);
  await openApp(page, { theme, data: data(reply({ assistantMessage: long })) });
  const field = await send(page, "plan");
  await field.getByRole("button", { name: "Expand" }).click();
  await field.getByLabel("Message Amara").focus();
  const list = field.getByRole("list", { name: "Messages" });
  await expect(list.getByText(/Line of the plan/)).toBeVisible();
  const composer = field.getByLabel("Message Amara");
  await expect(composer).toBeFocused();
  await composer.press("Home");
  await expect.poll(() => list.evaluate((el) => el.scrollTop)).toBe(0);
  await composer.press("End");
  await expect.poll(() => list.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  await composer.fill("draft");
  await list.evaluate((el) => { el.scrollTop = 200; });
  await composer.press("Home");
  expect(await list.evaluate((el) => el.scrollTop)).toBe(200);
  await composer.fill("");
  await composer.blur();
  await page.keyboard.press("Home");
  await expect.poll(() => list.evaluate((el) => el.scrollTop)).toBe(0);
  await page.keyboard.press("End");
  await expect.poll(() => list.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
});
