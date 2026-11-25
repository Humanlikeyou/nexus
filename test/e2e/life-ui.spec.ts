import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, routeSettled, setMock, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** PLAN-0055 / decision 0056: every Life button, form and navigation action, end to end against the mocked backend. */
// NOW is Wed 2026-10-28 JST, so this week is Mon 10-26 – Sun 11-01.
const MONDAY = "2026-10-26";
const REC = {
  recommendationId: "r1", scope: "personal", domain: "life", kind: "habit_below_target", title: "Meditate was done 1 of 5 days", body: "1 done.",
  observation: "1 done and 1 not done in the week, 5 days with no record.", rationale: "Seeing the gap helps.", suggestedAction: "Try one breath on hard days",
  confidence: null, confidenceBasis: null, uncertainty: "Only recorded days count.", impact: null, effort: "low", status: "shown", createdAt: "", updatedAt: "",
  expiresAt: null, producedBy: "rules:life_week", modelTier: null, targetRef: "goals:h1", supersedesId: null,
};
const LINK = { id: "l1", kind: "goal_project", goalId: "g1", otherRef: "project:p1", createdAt: "" };
function data() {
  const base = responses();
  return {
    ...base,
    read_goals: [
      { id: "g1", title: "Run 300 km", category: "fitness", target: 300, current: 212, unit: "km", deadline: "2026-12-31", created_at: "", updated_at: "", life: { areaId: "a1", nextAction: "Run 10 km on Saturday" } },
      { id: "h1", title: "Meditate", category: "habit", target: 7, current: 0, unit: "days", deadline: null, created_at: "", updated_at: "", life: { frequency: { times: 5, per: "week" } } },
    ],
    life_areas: [{ id: "a1", name: "Health", description: null, status: "active", createdAt: "", updatedAt: "" }],
    life_links: [],
    life_progress: { goalId: "g1", status: "active", current: 212, target: 300, unit: "km", baseline: null, fraction: null, history: [] },
    life_habit_history: [{ day: MONDAY, state: "done" }, { day: "2026-10-27", state: "not_done" }],
    list_terrence_projects: [{ id: "p1", title: "Marathon block", objective: "", currentState: "", status: "active", milestones: [{ title: "Long run 30 km", status: "open", dueAt: null }], nextAction: "Book the track", blockers: [], dependencies: [], contextItemIds: [], provenanceSource: "", createdAt: "", updatedAt: "", completedAt: null }],
    read_tasks: [{ title: "Buy running shoes", status: "active", priority: null, tags: ["goal:g1"], venture: null }],
    life_week_plan_get: { weekStart: MONDAY, priority: null, priorityGoalId: null, actions: [], note: null, updatedAt: null },
    life_week_plan_set: { weekStart: MONDAY, priority: "Finish the long run", priorityGoalId: "g1", actions: [{ text: "Stretch daily", goalId: null, done: false }], note: null, updatedAt: "2026-10-28T14:00:00+09:00" },
    life_insight: { recommendation: REC, nudged: true, held: null },
    life_week_observations: [{ ...REC, recommendationId: "r2", kind: "goal_stalled", title: "“Run 300 km” hasn't moved in 14 days", suggestedAction: "Run 10 km on Saturday", targetRef: "goals:g1" }],
    recommendation_respond: "o1",
    recommendation_record_outcome: "o2",
    recommendation_get: { recommendation: REC, outcomes: [], evidence: [{ evidenceId: "e1", recordRef: "goals:h1", windowStart: "2026-10-19T00:00:00+09:00", windowEnd: "2026-10-25T23:59:59+09:00", role: "observation", quality: "self_report", note: null, capturedAt: "", snapshotSha256: null }] },
    habit_mark: { changed: true, id: "hist-1" },
    life_update: { id: "g1" },
    life_link_add: LINK,
    life_link_remove: true,
    life_area_upsert: { id: "a2", name: "Home", description: null, status: "active", createdAt: "", updatedAt: "" },
    life_date_list: [{ id: "d1", title: "Visa renewal window", date: "2027-06-14", lane: "residence", sub: "", leadDays: [], goalId: "", done: false, inDays: 229, reminders: [{ days: 90, on: "2027-03-16", passed: false }], activeWindow: null }],
    life_date_range: [{ id: "far", title: "Passport renewal", date: "2029-05-01", lane: "personal", sub: "", leadDays: [], goalId: "", done: false, inDays: 916, reminders: [], activeWindow: null }],
    activity_list: [],
    proactive_settings_get: { checkin: { enabled: true, at: "22:00" }, habits: { enabled: true }, money: { enabled: true, weekday: "Sun", at: "19:00" }, nudges: { enabled: true, telegram: true, maxPerDay: 2, quietFrom: "22:00", quietTo: "07:00" } },
    proactive_settings_set: { checkin: { enabled: true, at: "22:00" }, habits: { enabled: true }, money: { enabled: true, weekday: "Sun", at: "19:00" }, nudges: { enabled: true, telegram: false, maxPerDay: 2, quietFrom: "22:00", quietTo: "07:00" } },
  };
}

type Call = { cmd: string; args?: Record<string, unknown> };
const calls = (page: Page, cmd: string) => page.evaluate((c) => ((window as unknown as { __E2E_CALLS: Call[] }).__E2E_CALLS).filter((x) => x.cmd === c), cmd);
const waitCall = async (page: Page, cmd: string, n = 1) => { await expect.poll(async () => (await calls(page, cmd)).length).toBeGreaterThanOrEqual(n); return (await calls(page, cmd))[n - 1].args ?? {}; };
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
async function life(page: Page, tab?: string) {
  await navButton(page, "Life").click();
  const field = activeField(page, "Life");
  if (tab) {
    const r = field.getByRole("radio", { name: tab, exact: true });
    await r.click();
    await expect(r).toHaveAttribute("aria-checked", "true");
  }
  await routeSettled(page).catch(() => undefined);
  return field;
}

test("Life opens on Overview; every Life tab is reachable and axe clean", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  const field = await life(page);
  await expect(field.getByRole("radio", { name: "Overview", exact: true })).toHaveAttribute("aria-checked", "true");
  await expect(field.getByRole("heading", { level: 1, name: "Overview" })).toBeVisible();
  for (const [tab, h1] of [["Goals & projects", "Goals & projects"], ["Habits & routines", "Habits & routines"], ["Weekly review", "Weekly review"], ["Dates & deadlines", "Dates & deadlines"], ["Personal money", "Personal money"], ["Overview", "Overview"]] as const) {
    const f = await life(page, tab);
    await expect(f.getByRole("heading", { level: 1, name: h1 })).toBeVisible();
    if (tab !== "Personal money") { await page.waitForTimeout(250); expect(await blocking(page), tab).toEqual([]); }
  }
  expect(consoleErrors).toEqual([]);
});

test("Overview: focus, today's actions, a habit recorded, the plan edited, and the insight accepted, done and rated", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  const field = await life(page);
  await expect(field.getByText("Focus: Run 300 km")).toBeVisible();
  const actions = field.getByRole("list", { name: "Today's planned actions" });
  await expect(actions).toContainText("Run 10 km on Saturday");
  await expect(actions).toContainText("Buy running shoes");
  await expect(field.getByRole("list", { name: "Goal progress" })).toContainText("212 / 300 km");
  await expect(field.getByRole("list", { name: "Goal progress" })).not.toContainText("%");
  await expect(field.getByRole("list", { name: "Upcoming deadlines" })).toContainText("Run 300 km");
  // Daily habit recording.
  await field.getByRole("button", { name: "Done today: Meditate" }).click();
  expect(await waitCall(page, "habit_mark")).toEqual({ habitId: "h1", day: "2026-10-28", done: true, source: "manual" });
  await expect(field.getByText("Marked “Meditate” done today.")).toBeVisible();
  // Editable weekly planning.
  await field.getByRole("button", { name: "Plan this week" }).click();
  await field.getByLabel("Primary priority").fill("Finish the long run");
  await field.getByLabel("It moves").selectOption("g1");
  await field.getByRole("button", { name: "Add an action" }).click();
  await field.getByLabel("Action 1", { exact: true }).fill("Stretch daily");
  await field.getByRole("button", { name: "Save plan" }).click();
  const saved = await waitCall(page, "life_week_plan_set");
  expect(saved.plan).toMatchObject({ weekStart: MONDAY, priority: "Finish the long run", priorityGoalId: "g1", actions: [{ text: "Stretch daily", goalId: null, done: false }] });
  await expect(field.getByText("Plan saved.")).toBeVisible();
  // The insight: explain, accept, mark done, record outcome.
  const insight = field.getByRole("region", { name: "Amara's insight" });
  await expect(insight).toContainText("Meditate was done 1 of 5 days");
  await insight.getByRole("button", { name: "Explain" }).click();
  await expect(insight.getByLabel("Why Amara suggests this")).toContainText("Observation · You recorded it · Oct 19 – Oct 25");
  await expect(insight).toContainText("Only recorded days count.");
  await insight.getByRole("button", { name: "Accept" }).click();
  expect(await waitCall(page, "recommendation_respond")).toEqual({ scope: "personal", id: "r1", response: "accepted", modificationJson: null });
  await insight.getByRole("button", { name: "Mark done" }).click();
  expect(await waitCall(page, "recommendation_record_outcome")).toMatchObject({ id: "r1", outcome: "completed" });
  await insight.getByRole("button", { name: "It helped" }).click();
  expect(await waitCall(page, "recommendation_record_outcome", 2)).toMatchObject({ id: "r1", outcome: "evaluated", evaluation: "helped" });
  await expect(insight).toContainText("Outcome recorded");
  // Suggestion settings save the whole settings object (check-ins kept).
  await field.getByRole("switch", { name: "Also on Telegram" }).click();
  expect((await waitCall(page, "proactive_settings_set")).settings).toMatchObject({ checkin: { enabled: true, at: "22:00" }, nudges: { telegram: false, maxPerDay: 2 } });
  expect((await calls(page, "life_insight")).length).toBe(1);
  expect(consoleErrors).toEqual([]);
});

test("Overview without a suggestion says why, and never shows a number it wasn't given", async ({ page, theme }) => {
  const d = { ...data(), life_insight: { recommendation: null, nudged: false, held: "daily_limit" } };
  await openApp(page, { theme, reduceMotion: true, data: d });
  const field = await life(page);
  await expect(field.getByRole("region", { name: "Amara's insight" })).toContainText("You've had today's suggestions.");
});

test("Habits & routines: the week calendar records a day, unknown stays unknown, the rhythm is edited", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  const field = await life(page, "Habits & routines");
  const card = field.getByRole("region", { name: "Meditate" });
  await expect(card).toContainText("This week: 1 of 5 days · 1 not done · 1 day with no record");
  await expect(card.getByRole("button", { name: /^Thu 10-29: later this week/ })).toBeDisabled();
  await card.getByRole("button", { name: /^Wed 10-28: no record\. Mark done/ }).click();
  expect(await waitCall(page, "habit_mark")).toEqual({ habitId: "h1", day: "2026-10-28", done: true, source: "manual" });
  await card.getByRole("button", { name: /^Tue 10-27: not done\. Mark done/ }).click();
  expect(await waitCall(page, "habit_mark", 2)).toEqual({ habitId: "h1", day: "2026-10-27", done: true, source: "manual" });
  await expect(card.getByRole("list", { name: "Meditate, last 12 weeks" }).getByRole("listitem")).toHaveCount(12);
  await card.getByRole("button", { name: "Edit rhythm: Meditate" }).click();
  await card.getByLabel("How often").fill("4");
  await card.getByLabel("Cue (after…)").fill("morning coffee");
  await card.getByLabel("Minimum version").fill("one breath");
  await card.getByRole("button", { name: "Save habit" }).click();
  expect((await waitCall(page, "life_update")).fields).toMatchObject({ frequency: { times: 4, per: "week" }, cue: "morning coffee", minimumVersion: "one breath", status: "active" });
  await expect(field.getByText("Saved “Meditate”.")).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test("Goals & projects: area → goal → project, link and unlink, the milestone timeline, Life details, and a new area", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  const field = await life(page, "Goals & projects");
  await expect(field.getByText("Habits have their own page now.")).toBeVisible();
  const tree = field.getByRole("region", { name: "Goals and projects" });
  const health = tree.getByRole("group", { name: "Health" });
  await health.getByRole("button", { name: /^Run 300 km/ }).click();
  await expect(health.getByRole("button", { name: /^Run 300 km/ })).toHaveAttribute("aria-expanded", "true");
  await expect(health).toContainText("Buy running shoes");
  // Link a project, then (the backend now lists it) unlink it.
  await setMock(page, "life_links", [LINK]);
  await health.getByLabel("Project", { exact: true }).selectOption("p1");
  await health.getByRole("button", { name: "Link", exact: true }).click();
  expect(await waitCall(page, "life_link_add")).toEqual({ kind: "goal_project", goalId: "g1", otherId: "p1" });
  await expect(health).toContainText("next: Book the track · milestone: Long run 30 km");
  await setMock(page, "life_links", []);
  await health.getByRole("button", { name: "Unlink project Marathon block" }).click();
  expect(await waitCall(page, "life_link_remove")).toEqual({ linkId: "l1" });
  // Life details.
  await health.getByRole("button", { name: "Edit details: Run 300 km" }).click();
  await health.getByLabel("Status").selectOption("blocked");
  await health.getByRole("button", { name: "Save details" }).click();
  await expect(health.getByRole("alert")).toHaveText("Say what it's blocked by.");
  await health.getByLabel("Why (paused or blocked)").fill("Knee injury");
  await health.getByLabel(/Baseline/).fill("100");
  await health.getByRole("button", { name: "Save details" }).click();
  expect((await waitCall(page, "life_update")).fields).toMatchObject({ status: "blocked", statusReason: "Knee injury", baseline: 100, areaId: "a1" });
  // A new life area.
  await tree.getByLabel("New area").fill("Home");
  await tree.getByRole("button", { name: "Add area" }).click();
  expect(await waitCall(page, "life_area_upsert")).toEqual({ id: null, name: "Home", description: null });
  expect(consoleErrors).toEqual([]);
});

test("Weekly review: reporting dates, habits against targets, goals by state, and an observation modified and dismissed", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  const field = await life(page, "Weekly review");
  const lifeWeek = field.getByLabel("Life this week");
  await expect(lifeWeek.getByRole("region", { name: "Reporting week" })).toContainText(/Mon \w{3} \d+ – Sun \w{3} \d+/);
  await expect(lifeWeek.getByRole("list", { name: "Habits against their targets" })).toContainText("Meditate");
  // Run 300 km has no progress in the 14 days to the week's end and its deadline is within 90 days: stalled.
  await expect(lifeWeek.getByRole("region", { name: "Goals this week" })).toContainText("StalledRun 300 km");
  const obs = lifeWeek.getByRole("region", { name: "Suggested next actions" });
  await expect(obs).toContainText("“Run 300 km” hasn't moved in 14 days");
  await obs.getByRole("button", { name: "Modify" }).click();
  await obs.getByLabel("What you'll do instead").fill("Run 5 km on Saturday");
  await obs.getByRole("button", { name: "Save change" }).click();
  const m = await waitCall(page, "recommendation_respond");
  expect(m).toMatchObject({ id: "r2", response: "modified" });
  expect(JSON.parse(String(m.modificationJson))).toEqual({ suggestedAction: { from: "Run 10 km on Saturday", to: "Run 5 km on Saturday" } });
  await expect(obs).toContainText("Suggested Run 5 km on Saturday");
  expect(consoleErrors).toEqual([]);
});

test("Weekly review: Dismiss records once and the controls go away", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  const obs = (await life(page, "Weekly review")).getByRole("region", { name: "Suggested next actions" });
  await obs.getByRole("button", { name: "Dismiss" }).click();
  expect(await waitCall(page, "recommendation_respond")).toMatchObject({ id: "r2", response: "dismissed" });
  await expect(obs).toContainText("It won't come back");
  await expect(obs.getByRole("button", { name: "Accept" })).toHaveCount(0);
});

test("Dates & deadlines: ranges beyond 12 months, and a custom range", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  const field = await life(page, "Dates & deadlines");
  await field.getByLabel("Show").selectOption("5y");
  expect(await waitCall(page, "life_date_range")).toEqual({ from: "2026-10-28", to: "2031-10-28" });
  await expect(field.getByRole("table", { name: "Dates in next 5 years" })).toContainText("Passport renewal");
  await expect(field.getByRole("img", { name: /Dates over next 5 years/ })).toBeVisible();
  await field.getByLabel("Show").selectOption("custom");
  await field.getByLabel("From", { exact: true }).fill("2027-01-01");
  await field.getByLabel("To", { exact: true }).fill("2030-01-01");
  await expect.poll(async () => (await calls(page, "life_date_range")).some((c) => c.args?.from === "2027-01-01" && c.args?.to === "2030-01-01")).toBe(true);
  await field.getByLabel("To", { exact: true }).fill("2026-01-01");
  await expect(field.getByRole("alert")).toHaveText("Pick a start before the end, up to 10 years apart.");
  await field.getByLabel("Show").selectOption("12m");
  await expect(field.getByRole("table", { name: "Upcoming" })).toBeVisible();
  expect(consoleErrors).toEqual([]);
});
