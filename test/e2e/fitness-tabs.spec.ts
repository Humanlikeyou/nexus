import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect, openApp, openView, test } from "./fixtures/tauri-mock";
import { NOOP_HYPNOGRAM, NOOP_SERIES_BY_KEY, NOOP_SNAPSHOT, NOOP_WORKOUTS, responses } from "./fixtures/snapshot";

/**
 * 0017: NOOP's Workouts, Sleep detail and Trends as tabs inside Mind-Body > Fitness.
 * Written to run with the existing fixtures; not run when authored (the parent runs e2e).
 */
const withNoop = (over: Record<string, unknown> = {}) => ({
  ...responses(),
  noop_recent_workouts: NOOP_WORKOUTS,
  noop_hypnogram: NOOP_HYPNOGRAM,
  noop_query: { __byTool: { health_snapshot: NOOP_SNAPSHOT, metric_series: { __byKey: NOOP_SERIES_BY_KEY } } },
  ...over,
});

async function openTab(page: Page, tab: string) {
  const field = await openView(page, "Fitness");
  const radio = field.getByRole("radiogroup", { name: "Fitness section" }).getByRole("radio", { name: tab, exact: true });
  await radio.click();
  await expect(radio).toHaveAttribute("aria-checked", "true");
  return field;
}

async function blocking(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  return results.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 4).map((n) => n.target.join(" ")).join(" | ")}`);
}

test.describe("Fitness tabs (0017)", () => {
  test("tab bar: four sections, Summary first, keyboard reachable", async ({ page, theme }) => {
    await openApp(page, { theme, data: withNoop(), reduceMotion: true });
    const field = await openView(page, "Fitness");
    const group = field.getByRole("radiogroup", { name: "Fitness section" });
    await expect(group.getByRole("radio")).toHaveCount(4);
    await expect(group.getByRole("radio", { name: "Summary", exact: true })).toHaveAttribute("aria-checked", "true");
    await group.getByRole("radio", { name: "Summary", exact: true }).focus();
    await page.keyboard.press("ArrowRight");
    await expect(group.getByRole("radio", { name: "Workouts", exact: true })).toHaveAttribute("aria-checked", "true");
  });

  test("Workouts: sessions with zone minutes, and an honest line for no zones", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme, data: withNoop(), reduceMotion: true });
    const f = await openTab(page, "Workouts");
    await expect(f.getByRole("list", { name: "Workouts" })).toBeVisible();
    await expect(f.getByRole("heading", { level: 3, name: "Running" })).toBeVisible();
    await expect(f.getByRole("img", { name: "Running heart-rate zones: Zone 1 4m, Zone 2 12m, Zone 3 16m, Zone 4 10m, Zone 5 3m" })).toBeVisible();
    await expect(f.getByText("No zone data for this session.")).toBeVisible();
    await expect(f.getByText("Sample data")).toHaveCount(0);
    expect(await blocking(page)).toEqual([]);
    expect(consoleErrors).toEqual([]);
  });

  test("Workouts: empty and error states", async ({ page, theme }) => {
    await openApp(page, { theme, data: withNoop({ noop_recent_workouts: [] }), reduceMotion: true });
    await expect((await openTab(page, "Workouts")).getByText("No workouts yet.")).toBeVisible();
  });
  test("Workouts: a failed read is a plain sentence", async ({ page, theme }) => {
    await openApp(page, { theme, data: withNoop({ noop_recent_workouts: { __reject: "NOOP store could not be opened: /Users/x/db.sqlite" } }), reduceMotion: true });
    const f = await openTab(page, "Workouts");
    await expect(f.getByRole("alert")).toContainText("couldn't read workouts");
    await expect(f.getByText(/sqlite/)).toHaveCount(0);
    await expect(f.getByRole("button", { name: "Retry" })).toBeVisible();
  });
  test("Workouts: no store reads as Needs wiring", async ({ page, theme }) => {
    await openApp(page, { theme, data: withNoop({ noop_recent_workouts: { __reject: "NOOP has not created its local health store yet." } }), reduceMotion: true });
    await expect((await openTab(page, "Workouts")).getByText("Needs wiring")).toBeVisible();
  });

  test("Sleep detail: hypnogram with text alternative, stages, 14 nights", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme, data: withNoop(), reduceMotion: true });
    const f = await openTab(page, "Sleep detail");
    await expect(f.getByRole("img", { name: /^Sleep stages from 23:41 to 07:13:/ })).toBeVisible();
    await expect(f.getByRole("img", { name: /^Time in each sleep stage:/ })).toBeVisible();
    await expect(f.getByRole("img", { name: /^Time asleep, last 14 nights/ })).toBeVisible();
    await f.getByText("Stage timeline as text").click();
    await expect(f.getByRole("listitem").filter({ hasText: "Deep, 1h 10m" }).first()).toBeVisible();
    await f.getByRole("combobox", { name: "Night to show", exact: true }).selectOption({ index: 2 });
    await expect(f.getByRole("img", { name: /^Sleep stages/ })).toBeVisible();
    expect(await blocking(page)).toEqual([]);
    expect(consoleErrors).toEqual([]);
  });
  test("Sleep detail: no segments is an honest empty state", async ({ page, theme }) => {
    await openApp(page, { theme, data: withNoop({ noop_hypnogram: null }), reduceMotion: true });
    const f = await openTab(page, "Sleep detail");
    await expect(f.getByText("No sleep stages yet.")).toBeVisible();
    await expect(f.getByRole("img", { name: /^Sleep stages/ })).toHaveCount(0);
  });

  test("Trends: all 9 metrics, range and metric change the chart, no extra reads", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme, data: withNoop(), reduceMotion: true });
    const f = await openTab(page, "Trends");
    await expect(f.getByRole("img", { name: /^Recovery, last 30 days/ })).toBeVisible();
    await expect(f.getByRole("combobox", { name: "Metric", exact: true }).locator("option")).toHaveCount(9);
    await expect(f.getByRole("list", { name: "All metrics" }).getByRole("button")).toHaveCount(9);
    await f.getByRole("radiogroup", { name: "Trend range" }).getByRole("radio", { name: "Week", exact: true }).click();
    await expect(f.getByRole("img", { name: /^Recovery, last 7 days/ })).toBeVisible();
    await f.getByRole("list", { name: "All metrics" }).getByRole("button", { name: /^Steps/ }).click();
    await expect(f.getByRole("img", { name: /^Steps, last 7 days/ })).toBeVisible();
    await expect(f.getByRole("list", { name: "All metrics" }).getByRole("button", { name: /^Steps/ })).toHaveAttribute("aria-pressed", "true");
    expect(await blocking(page)).toEqual([]);
    expect(consoleErrors).toEqual([]);
  });
  test("Trends: a failed read is a plain sentence", async ({ page, theme }) => {
    // No metric_series answer: the mock rejects every key with tool_not_allowed.
    await openApp(page, { theme, data: withNoop({ noop_query: { __byTool: { health_snapshot: NOOP_SNAPSHOT } } }), reduceMotion: true });
    const f = await openTab(page, "Trends");
    await expect(f.getByRole("alert")).toContainText("couldn't read trends");
    await expect(f.getByRole("button", { name: "Retry" })).toBeVisible();
  });
  test("Trends: helper not set up reads as Needs wiring", async ({ page, theme }) => {
    const reject = { kind: "binary_missing", detail: "x" };
    await openApp(page, { theme, data: withNoop({ noop_query: { __reject: reject } }), reduceMotion: true });
    await expect((await openTab(page, "Trends")).getByText("Needs wiring")).toBeVisible();
  });
});
