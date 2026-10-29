import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

const at = (iso: string) => Date.parse(iso);

/** Chat & Agents › Activity: kill-switch Suppressed (neutral), outbox Expired row, prune archive count. */
test("activity shows Suppressed as neutral, the Expired outbox row and the activity-archive count", async ({ page, theme, consoleErrors }) => {
  const base = responses() as Record<string, unknown>;
  await openApp(page, {
    theme,
    data: {
      ...base,
      get_outbox_status: { ...(base.get_outbox_status as object), expired: 3 },
      list_activity_entries: [
        { id: "s1", type: "ritual", name: "morning_ritual", status: "suppressed", startedAt: at("2026-10-29T07:00:00+09:00"), completedAt: at("2026-10-29T07:00:01+09:00"), durationMs: 300, detail: "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)", outcome: "suppressed" },
        { id: "p1", type: "maintenance", name: "memory_prune", status: "completed", startedAt: at("2026-10-29T06:00:00+09:00"), completedAt: at("2026-10-29T06:00:05+09:00"), durationMs: 5000, detail: "2 sessions archived, 1 trimmed, 16589 activity entries archived", outcome: null },
        { id: "f1", type: "agent_task", name: "daily_brief", status: "failed", startedAt: at("2026-10-28T07:30:00+09:00"), completedAt: at("2026-10-28T07:30:42+09:00"), durationMs: 42000, detail: "Delivery failed", outcome: null },
      ],
    },
  });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await field.getByRole("radio", { name: "Activity" }).click();
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Activity");

  const expired = field.getByRole("listitem").filter({ hasText: "Expired" }).first();
  await expect(expired).toContainText("Older than 24h, kept, not sent");
  await expect(expired).toContainText("3");

  const suppressed = field.getByRole("listitem").filter({ hasText: "morning ritual" });
  await expect(suppressed).toContainText("Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)");
  const badge = suppressed.locator(".ui-badge");
  await expect(badge).toHaveText("Suppressed");
  await expect(badge).toHaveAttribute("data-tone", "neutral");

  await expect(field.getByRole("listitem").filter({ hasText: "memory prune" })).toContainText("16589 activity entries archived");
  await expect(field.getByRole("listitem").filter({ hasText: "daily brief" }).locator(".ui-badge")).toHaveAttribute("data-tone", "danger");
  expect(consoleErrors).toEqual([]);
});

test("no Expired row when nothing has expired", async ({ page, theme }) => {
  await openApp(page, { theme });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await field.getByRole("radio", { name: "Activity" }).click();
  await expect(field.getByText("Last delivered")).toBeVisible();
  await expect(field.getByText("Older than 24h, kept, not sent")).toHaveCount(0);
});
