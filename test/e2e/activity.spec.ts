import { activeField, expect, navButton, openApp, test, openMore } from "./fixtures/tauri-mock";
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
        { id: "k1", type: "daily_summary", name: "weekly_review", status: "skipped", startedAt: at("2026-10-27T21:40:00+09:00"), completedAt: at("2026-10-27T21:40:00+09:00"), durationMs: 0, detail: "Missed window", outcome: "skipped" },
      ],
    },
  });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await field.getByRole("radio", { name: "Activity" }).click();
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Activity");
  await openMore(page);

  const expired = field.getByRole("listitem").filter({ hasText: "Expired" }).first();
  await expect(expired).toContainText("Older than 24h, kept, not sent");
  await expect(expired).toContainText("3");

  const suppressed = field.getByRole("listitem").filter({ hasText: "morning ritual" });
  await expect(suppressed).toContainText("Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)");
  const badge = suppressed.locator(".ui-badge");
  await expect(badge).toHaveText("Suppressed");
  await expect(badge).toHaveAttribute("data-tone", "neutral");

  await expect(field.getByRole("listitem").filter({ hasText: "memory prune" })).toContainText("16589 activity entries archived");
  const failed = field.getByRole("listitem").filter({ hasText: "daily brief" });
  await expect(failed.locator(".ui-badge")).toHaveAttribute("data-tone", "danger");
  await expect(failed.locator(".ui-badge")).toHaveText("Failed");
  // One timestamp format: "Oct 28, 7:30 AM" (no " at ", no leading zero).
  await expect(failed).toContainText("42.0s · Oct 28, 7:30 AM");

  const skipped = field.getByRole("listitem").filter({ hasText: "weekly review" });
  await expect(skipped.locator(".ui-badge")).toHaveText("Skipped");
  await expect(skipped.locator(".ui-badge")).toHaveAttribute("data-tone", "neutral");
  await expect(field.getByText(/^Last delivered /)).toContainText("Oct 28, 1:40 PM");
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

/** Rituals whose 30-min window passed are stored as status "skipped" (neutral), one row each. */
test("skipped rituals render one neutral 'Skipped' row each, labelled Ritual", async ({ page, theme, consoleErrors }) => {
  const base = responses() as Record<string, unknown>;
  const skipped = (id: string, name: string, iso: string) => ({
    id, type: "ritual", name: `Ritual: ${name}`, status: "skipped", startedAt: at(iso), completedAt: at(iso), durationMs: 0,
    detail: "Skipped (missed window)", outcome: "skipped",
  });
  await openApp(page, {
    theme,
    data: {
      ...base,
      list_activity_entries: [
        skipped("r1", "Presence Drill", "2026-10-29T10:39:00+09:00"),
        skipped("r2", "Command Phrase + Intent", "2026-10-29T10:39:00+09:00"),
        skipped("r3", "Alpha Breathing", "2026-10-29T10:39:00+09:00"),
      ],
    },
  });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await field.getByRole("radio", { name: "Activity" }).click();
  const rows = field.getByRole("listitem").filter({ hasText: "Skipped (missed window)" });
  await expect(rows).toHaveCount(3);
  for (const row of await rows.all()) {
    await expect(row).toContainText("Ritual");
    const badge = row.locator(".ui-badge");
    await expect(badge).toHaveText("Skipped");
    await expect(badge).toHaveAttribute("data-tone", "neutral");
  }
  expect(consoleErrors).toEqual([]);
});
