import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, openMore, openView, routeSettled, setMock, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** design-1110e: Neuro recordings (E1–E8, E11, E12) and the live signal (E9), sample adapters off and on. */
async function blocking(page: Page, scope?: string) {
  const b = new AxeBuilder({ page });
  const r = await (scope ? b.include(scope) : b).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const field = (p: Page) => activeField(p, "Mind–Body");
const STATES: Array<[string, (p: Page) => Promise<void>]> = [
  ["list", async () => {}],
  ["detail", async (p) => { const b = field(p).getByRole("table", { name: "Recordings" }).getByRole("button"); await ((await b.count()) > 1 ? b.nth(1) : b.first()).click(); }],
  ["retention dialog", async (p) => { const b = field(p).getByRole("button", { name: /^Review \d/ }); if (await b.count()) await b.click(); }],
];

for (const sample of [false, true]) {
  for (const [name, go] of STATES) {
    test(`(1110e) Neuro history ${name}${sample ? " with sample adapters" : ""}: axe clean (incl. 4.5:1 contrast), honest copy`, async ({ page, theme }) => {
      if (sample) await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
      await openApp(page, { theme, reduceMotion: true });
      await openView(page, "Neuro history");
      await routeSettled(page).catch(() => undefined);
      await go(page);
      await page.waitForTimeout(300);
      expect(await blocking(page)).toEqual([]);
      await expect(page.getByText(/Graphs over time aren.t recorded yet|which it doesn.t keep yet|per-minute samples/)).toHaveCount(0);
      if (sample) await expect(page.getByText("Sample data").first()).toBeVisible();
    });
  }
}

test("(1110e) retention: Keep all removes nothing and the list is unchanged", async ({ page, theme }) => {
  await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true });
  await openView(page, "Neuro history");
  const f = field(page);
  await expect(f.getByText(/Over 90 days · needs your OK/)).toHaveCount(3);
  await f.getByRole("button", { name: "Review 3" }).click();
  await page.getByRole("button", { name: "Keep all" }).click();
  await expect(f.getByText(/Over 90 days · needs your OK/)).toHaveCount(3);
  await expect(page.getByRole("status").filter({ hasText: /deleted/ })).toHaveCount(0);
});

test("(1110e) live signal: stacked lanes for every channel, 10 s window, AUX lane when detected", async ({ page, theme }) => {
  await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true, data: { ...responses(), get_muse_state: { connected: true, device_name: "Muse 2", streaming: false }, start_eeg_recording: "/tmp/mock.bin", start_eeg_stream: null, stop_eeg_stream: null, stop_eeg_recording: null, save_session: "/tmp/s.json", load_kv: null, save_kv: null } });
  await navButton(page, "Mind–Body").click();
  await field(page).getByRole("radio", { name: "Neuro", exact: true }).click();
  await setMock(page, "get_muse_state", { connected: true, device_name: "Muse 2", streaming: true });
  await field(page).getByRole("button", { name: "Start session" }).click();
  await page.evaluate(() => {
    const w = window as unknown as { __E2E_EMIT: (e: string, p: unknown) => void; __n: number };
    w.__n = 0;
    setInterval(() => { const s = w.__n; w.__n += 12; const g = (k: number) => Array.from({ length: 12 }, (_, i) => 14 * Math.sin(2 * Math.PI * 10 * ((s + i) / 256) + k));
      for (const [i, c] of ["TP9", "AF7", "AF8", "TP10", "AUX"].entries()) w.__E2E_EMIT("muse-eeg", { timestamp: Date.now(), channel: c, samples: g(i) }); }, 47);
  });
  await page.clock.runFor(3_000);
  await openMore(page);
  const card = field(page).getByRole("region", { name: "Live signal · µV" });
  for (const c of ["TP9", "AF7", "AF8", "TP10"]) await expect(card.getByRole("img", { name: new RegExp(`^${c} raw EEG, last 10 seconds`) })).toBeVisible();
  await expect(card.getByRole("img", { name: /AUX occipital raw EEG/ })).toBeVisible();
  await expect(card.getByRole("radio", { name: "10 s" })).toHaveAttribute("aria-checked", "true");
  // Scoped to the new card: the training meters around it are 1110c's and covered by neuro-flow.
  expect(await blocking(page, ".rec-live")).toEqual([]);
});
