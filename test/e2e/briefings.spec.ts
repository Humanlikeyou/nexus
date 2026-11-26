import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, routeSettled, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

// Decision 0066: Life › Briefings, the weekly review's cross-domain part, and Enterprise › Workspaces ›
// Briefing in the built app (mocked backend), both themes.
const PLAN = { metric: "workout.done_sets_7d", params: {}, baseline: 30, target: 9, direction: "at_most", unit: "sets", measure_after: "2026-12-03", window_days: 7, how: "Sets logged in the workout engine." };
const REC = {
  recommendationId: "x1", scope: "personal", domain: "fitness", kind: "training_volume_rising", title: "Training volume is 3.8× your recent weekly average", body: "",
  observation: "You logged 30 sets in 1 session.", rationale: "Volume is rising much faster than your recent weeks.", suggestedAction: "Keep next week at or below 9 sets.",
  confidence: 0.6, confidenceBasis: "3 earlier weeks", uncertainty: "Only sessions logged in the workout engine are counted.", impact: null, effort: "low",
  status: "proposed", createdAt: "", updatedAt: "", expiresAt: null, producedBy: "rules:cross_domain", modelTier: null, targetRef: null, supersedesId: null,
  evaluationJson: JSON.stringify(PLAN), periodStart: "2026-10-30T00:00:00+09:00", periodEnd: "2026-11-26T23:59:59+09:00", fingerprint: "training_volume_rising:abc",
};
const EVIDENCE = [{ evidenceId: "e1", recordRef: "workout:w5", windowStart: "2026-11-20T00:00:00+09:00", windowEnd: "2026-11-26T23:59:59+09:00", role: "observation", quality: "measured", note: null, capturedAt: "", snapshotSha256: "x" }];
const job = (id: string, label: string, schedule: string) => ({
  id, label, schedule, nextDueAt: "2026-10-29T07:00:00+09:00",
  state: { lastDay: "2026-10-28", lastRunAt: "2026-10-28T08:00:20+09:00", lastTrigger: "catch_up", lastResult: "ok", lastError: null, ruleCounts: { created: 1 }, measured: 0 },
});
const TRAINING = { periodStart: "2026-10-22", periodEnd: "2026-10-28", sessions: 1, doneSets: 30, totalReps: 300, activeMin: 40, baselineSetsPerWeek: 8, setsByMuscle: { chest: 30 }, detectedWorkoutMin: 55, meanStrain: 11.2, sourceIds: ["workout:w5"], note: "Sessions are the ones you logged in the workout engine." };
const NEURO = { periodStart: "2026-10-22", periodEnd: "2026-10-28", sessions: 2, minutes: 24, eligibleTraining: 1, medianTimeInTargetPct: 41, medianValidPct: 88, trends: [], sourceIds: ["session:s1"], note: "Muse EEG figures describe the recorded signal and the training protocol's targets. They are not a measure of mood, ability or mental health." };

function data() {
  return {
    ...responses(),
    briefing_runtime: {
      runsWhileClosed: false, timerStartedAt: "2026-10-28T07:59:40+09:00", modelInference: "None. Briefings and suggestions are fixed rules and arithmetic over your records.",
      note: "Briefings are built by Amara OS while it is open. When it is closed nothing runs; the first minute after the next launch builds anything that was missed.",
      jobs: [job("personal", "Daily personal briefing", "Every day at 07:00 JST while Amara OS is open"), job("enterprise", "Enterprise briefing", "Every day at 07:30 JST while Amara OS is open")],
    },
    briefing_daily_read: {
      day: "2026-10-28", builtAt: "2026-10-28T08:00:20+09:00", trigger: "catch_up", method: "Fixed rules and arithmetic over your records. No language model was used.",
      missing: [], openRecommendations: ["x1"], training: TRAINING, neuro: NEURO,
      trajectories: [{ goalId: "g1", title: "Akiya offer", measure: "1 of 10 steps", progressPct: 10, expectedPct: 57, deadline: "2027-01-31", daysLeft: 66, moves14d: 0, state: "behind", nextStep: "Call the agent", sources: ["goals:g1"] }],
      sections: [
        { id: "today", title: "Today and the week ahead", state: "data", note: "No calendar is connected, so only Life dates are listed.", lines: [{ text: "Visa renewal is due tomorrow.", kind: "self_report", sources: ["date:d1"] }] },
        { id: "body", title: "Sleep and recovery", state: "data", note: "WHOOP figures are measurements from the strap, read through NOOP. They are not a readiness, health or psychological assessment.", lines: [{ text: "2026-10-28: 6.4 h sleep (your 28-day median 7.2 h).", kind: "measured", sources: ["daily:2026-10-28"] }] },
        { id: "money", title: "Personal money", state: "unavailable", note: "Nothing logged in the money store; no bank account is connected.", lines: [] },
      ],
    },
    briefing_daily_refresh: { __reject: "Built less than a minute ago. Try again shortly." },
    briefing_recommendations: [{ recommendation: REC, evidence: EVIDENCE, outcomes: [] }],
    recommendation_get: { recommendation: REC, evidence: EVIDENCE, outcomes: [] },
    recommendation_respond: "o1",
    briefing_enterprise_read: {
      businessId: "taiyo-skin", name: "TaiyoSkin", day: "2026-10-28", builtAt: "", trigger: "opened", method: "Fixed rules and arithmetic over this business's records only. No language model was used.",
      missing: [], openRecommendations: [],
      sections: [
        { id: "work", title: "Work recorded this week", state: "none", lines: [], note: "No workspace work recorded from 2026-10-22 to 2026-10-28." },
        { id: "search_needs", title: "Search interest and customer needs", state: "unavailable", lines: [], note: "Search interest is unavailable: Search Console is deferred (decision 0064), so search queries aren't read. Nothing is estimated." },
      ],
    },
    business_list: [{ businessId: "taiyo-skin", name: "TaiyoSkin", description: null, ventureId: null, status: "active", createdAt: "", updatedAt: "" }],
  };
}

async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}

async function briefings(page: Page, theme: "light" | "dark") {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  await navButton(page, "Life").click();
  const field = activeField(page, "Life");
  const tab = field.getByRole("radio", { name: "Briefings", exact: true });
  await tab.click();
  await expect(tab).toHaveAttribute("aria-checked", "true");
  await routeSettled(page).catch(() => undefined);
  return field;
}

test("Life › Briefings: every section, the runtime said plainly, and a suggestion with all six parts", async ({ page, theme, consoleErrors }) => {
  const field = await briefings(page, theme);
  await expect(field.getByRole("heading", { name: "Your briefing for 2026-10-28" })).toBeVisible();
  await expect(field.getByText("Visa renewal is due tomorrow.")).toBeVisible();
  await expect(field.getByText(/not a readiness, health or psychological assessment/)).toBeVisible();
  await expect(field.getByText("Nothing logged in the money store; no bank account is connected.")).toBeVisible();
  await expect(field.getByText(/When it is closed nothing runs/)).toBeVisible();
  await expect(field.getByText(/Next due .*, if Amara OS is open then\./)).toBeVisible();
  await expect(field.getByText(/Behind pace/)).toBeVisible();
  await expect(field.getByText(/WHOOP detected 55 workout minutes/)).toBeVisible();
  await expect(field.getByText(/Median time in target: 41%/)).toBeVisible();
  const card = field.locator("section", { has: page.getByText("Training volume is 3.8× your recent weekly average") }).last();
  await expect(card.getByText(/At most 9 sets \(was 30\), checked after 2026-12-03/)).toBeVisible();
  await card.getByRole("button", { name: "Explain" }).click();
  for (const k of ["Reporting period", "Interpretation", "Confidence", "Limitations"]) await expect(card.getByText(k, { exact: true })).toBeVisible();
  await card.getByRole("button", { name: "Accept" }).click();
  await expect.poll(() => page.evaluate(() => (window as unknown as { __E2E_CALLS: Array<{ cmd: string; args?: Record<string, unknown> }> }).__E2E_CALLS
    .some((c) => c.cmd === "recommendation_respond" && c.args?.scope === "personal" && c.args?.response === "accepted"))).toBe(true);
  // A rebuild inside a minute is refused, and the message says why.
  await field.getByRole("button", { name: "Rebuild now" }).click();
  await expect(field.getByText("Built less than a minute ago. Try again shortly.")).toBeVisible();
  await page.waitForTimeout(250);
  expect(await blocking(page)).toEqual([]);
  expect(consoleErrors).toEqual([]);
});

test("Weekly review: the cross-domain part adds to the review without repeating it", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  await navButton(page, "Life").click();
  const field = activeField(page, "Life");
  await field.getByRole("radio", { name: "Weekly review", exact: true }).click();
  await expect(field.getByRole("heading", { name: "Across your domains" })).toBeVisible();
  await expect(field.getByText("No cross-domain findings for this week.")).toBeVisible();
  await expect(field.getByRole("heading", { name: "Neurotechnology this week" })).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

test("Workspaces › Briefing: business only, search honestly unavailable, axe clean", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme, reduceMotion: true, data: data() });
  await navButton(page, "Enterprise").click();
  const field = activeField(page, "Enterprise");
  await field.getByRole("radio", { name: "Workspaces", exact: true }).click();
  await field.getByRole("radio", { name: "Briefing", exact: true }).click();
  await expect(field.getByRole("heading", { name: "Briefing for 2026-10-28" })).toBeVisible();
  await expect(field.getByText(/Search Console is deferred/)).toBeVisible();
  await expect(field.getByText(/Every day at 07:30 JST while Amara OS is open/)).toBeVisible();
  const asked = await page.evaluate(() => (window as unknown as { __E2E_CALLS: Array<{ cmd: string; args?: Record<string, unknown> }> }).__E2E_CALLS
    .filter((c) => c.cmd === "briefing_recommendations").map((c) => c.args));
  expect(asked).toContainEqual({ scope: "business", businessId: "taiyo-skin" });
  await page.waitForTimeout(250);
  expect(await blocking(page)).toEqual([]);
  expect(consoleErrors).toEqual([]);
});
