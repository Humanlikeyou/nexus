import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** QA 11-04: action cards live in the thread flow; resolved ones are compact; stale ledger rows stay hidden. */
const CONV = "nexus-e2e-current";

const interaction = (i: number, tier: unknown = { tier: "fast", reason: "classifier_easy", source: "router_log" }) => {
  const at = new Date(Date.UTC(2026, 9, 28, 4, 0) + i * 60_000).toISOString();
  return {
    id: `i${i}`, conversationId: CONV, hermesSessionId: "api-e2e", runtime: "hermes", model: "amara", provider: "custom", status: "succeeded",
    userMessage: `question ${i}`, assistantMessage: `answer ${i}`, error: null, startedAt: at, completedAt: at, durationMs: 1000,
    contextItemIds: [], actionId: null, runtimeProfile: "nexus-managed-actions-v5", tier,
  };
};
const action = (id: string, title: string, status: string, conversationId: string | null, minute: number) => {
  const at = new Date(Date.UTC(2026, 9, 28, 4, 0) + minute * 60_000 + 30_000).toISOString();
  return {
    id, toolId: "system.open_application", title, status, requiresApproval: true, parameters: {}, conversationId, contextItemIds: [],
    requestedAt: at, updatedAt: at, approvedAt: null, startedAt: null, completedAt: null, result: null, error: null, retryOfActionId: null,
    retryAllowed: false, summary: "Launches an installed macOS application by its name.",
  };
};

async function open(page: import("@playwright/test").Page, theme: "light" | "dark", count: number, actions: unknown[]) {
  await page.addInitScript((id) => localStorage.setItem("nexus.terrence.conversationId", id), CONV);
  const base = responses();
  const health = { ...(base.get_terrence_runtime_health as object), model: "amara", routedViaRouter: true };
  await openApp(page, { theme, data: { ...base, get_terrence_runtime_health: health, list_terrence_interactions: Array.from({ length: count }, (_, i) => interaction(i)), list_terrence_actions: actions } });
  await navButton(page, "Chat & Agents").click();
  return activeField(page, "Chat & Agents");
}

test("cancelled cards are compact lines in the flow; the reply and tier badge aren't covered", async ({ page, theme, consoleErrors }) => {
  const f = await open(page, theme, 2, [
    action("s", "Open Safari", "cancelled", CONV, 1),
    action("v", "Lower volume by 10%", "cancelled", CONV, 1),
  ]);
  const list = f.getByRole("list", { name: "Messages" });
  const line = list.locator(".chat-action-line", { hasText: "Open Safari" });
  await expect(line).toContainText("Cancelled");
  await expect(list.getByRole("article")).toHaveCount(0);
  await expect(f.getByLabel("Actions")).toHaveCount(0);
  const reply = list.locator(".chat-bubble", { hasText: "answer 1" });
  await expect(reply).toBeVisible();
  await reply.scrollIntoViewIfNeeded();
  // The reply's centre must hit the reply itself (nothing overlays it).
  const hit = await reply.evaluate((el) => { const r = el.getBoundingClientRect(); return el.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)); });
  expect(hit).toBe(true);
  await expect(list.getByLabel("Answered on the Fast tier").last()).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test("pending card is prominent in the thread; the list keeps its height and scrolls through all history", async ({ page, theme }) => {
  const f = await open(page, theme, 24, [
    ...["a", "b", "c", "d"].map((id, i) => action(id, `Old action ${id}`, "cancelled", CONV, 20 + i)),
    action("p", "Open Notes", "awaiting_approval", CONV, 23),
  ]);
  const list = f.getByRole("list", { name: "Messages" });
  const card = list.getByRole("article").filter({ hasText: "Open Notes" });
  await expect(card.getByRole("button", { name: "Approve" })).toBeVisible();
  const [listBox, convBox] = await Promise.all([list.boundingBox(), f.locator(".chat-screen__conversation").boundingBox()]);
  expect(listBox!.height).toBeGreaterThan(convBox!.height * 0.5);
  await list.evaluate((el) => { el.scrollTop = 0; });
  await expect(list.getByText("question 0", { exact: true })).toBeInViewport();
  await list.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await expect(card).toBeInViewport();
});

test("stale ledger rows from other conversations (e.g. Open https://example.com) never show", async ({ page, theme }) => {
  const f = await open(page, theme, 1, [
    action("x", "Open https://example.com", "cancelled", "nexus-old-august", 0),
    action("y", "Open https://example.com", "succeeded", "nexus-old-august", 0),
  ]);
  const list = f.getByRole("list", { name: "Messages" });
  await expect(list.getByText("answer 0")).toBeVisible();
  await expect(f.getByText("Open https://example.com")).toHaveCount(0);
});
