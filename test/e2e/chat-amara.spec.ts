import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import { NOW, responses } from "./fixtures/snapshot";

/** Chat & Agents › Chat: Amara over Hermes, Fast/Deep badge, Deep toggle. AMARA_SHOTS=<dir> saves screenshots. */
const shot = async (page: import("@playwright/test").Page, name: string) => {
  if (process.env.AMARA_SHOTS) await page.screenshot({ path: `${process.env.AMARA_SHOTS}/${name}.png`, fullPage: true });
};

const interaction = (id: string, user: string, reply: string, at: string, tier: unknown) => ({
  id, conversationId: "c", hermesSessionId: "api-e2e", runtime: "hermes", model: "amara", provider: "custom", status: "succeeded",
  userMessage: user, assistantMessage: reply, error: null, startedAt: at, completedAt: at, durationMs: 4200,
  contextItemIds: [], actionId: null, runtimeProfile: "nexus-managed-actions-v5", tier,
});

const routed = () => {
  const base = responses();
  return {
    ...base,
    get_terrence_runtime_health: { ...(base.get_terrence_runtime_health as object), model: "amara", routedViaRouter: true },
    list_terrence_interactions: [
      interaction("i1", "What's on my plate this afternoon?", "Two things: finish the release notes, then review the sleep pipeline.", "2026-10-28T13:12:00+09:00", { tier: "fast", reason: "classifier_easy", source: "router_log" }),
      interaction("i2", "/deep How should I sequence the akiya purchase and the Utsunomiya move?", "Start with the inspection, then lock financing, then set the move date around the closing.", "2026-10-28T13:20:00+09:00", { tier: "deep", reason: "slash_deep", source: "router_log" }),
    ],
    prepare_amara_turn: { sections: [{ tag: "nexus_priorities", text: "1. Ship TaiyoSkin" }], itemIds: [] },
    interact_with_terrence: interaction("i3", "/deep plan my week", "Here's a plan for the week, front-loading TaiyoSkin.", NOW, { tier: "deep", reason: "slash_deep", source: "router_log" }),
  };
};

test("Chat: Amara's replies carry the router tier; Deep toggle sends a deep turn", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, data: routed() });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  const list = field.getByRole("list", { name: "Messages" });
  await expect(list.getByLabel("Answered on the Fast tier")).toHaveText("Fast");
  await expect(list.getByLabel("Answered on the Deep tier")).toHaveText("Deep");

  const deep = field.getByRole("button", { name: "Deep" });
  await expect(deep).toBeEnabled();
  await deep.click();
  await expect(deep).toHaveAttribute("aria-pressed", "true");
  await field.getByLabel("Message Amara").fill("plan my week");
  await shot(page, `chat-deep-toggle-${theme}`);
  await field.getByLabel("Message Amara").press("Enter");
  await expect(list.getByText("Here's a plan for the week")).toBeVisible();
  await expect(list.getByLabel("Answered on the Deep tier")).toHaveCount(2);
  await shot(page, `chat-tiers-${theme}`);
  expect(consoleErrors).toEqual([]);
});

test("Chat: no badge and Deep disabled while Hermes is not on the Amara Router", async ({ page, theme, consoleErrors }) => {
  const data = routed();
  data.get_terrence_runtime_health = { ...(data.get_terrence_runtime_health as object), routedViaRouter: false };
  data.list_terrence_interactions = (data.list_terrence_interactions as Array<Record<string, unknown>>).map((i) => ({ ...i, tier: null }));
  await openApp(page, { theme, data });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await expect(field.getByText("Two things: finish the release notes")).toBeVisible();
  await expect(field.getByRole("list", { name: "Messages" }).getByText(/^(Fast|Deep)$/)).toHaveCount(0);
  await expect(field.getByRole("button", { name: "Deep" })).toHaveAttribute("aria-disabled", "true");
  await shot(page, `chat-not-routed-${theme}`);
  expect(consoleErrors).toEqual([]);
});
