import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** 1112n: the Neuro redesign and the Hemi-Sync section, in every state the mock can reach (no live app, no real audio). */
async function blocking(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`);
}
const museData = (connected: boolean) => ({
  ...responses(),
  get_muse_state: { connected, device_name: "Muse 2", streaming: false },
  start_eeg_recording: "/tmp/mock-recording.bin", start_eeg_stream: null, stop_eeg_stream: null, stop_eeg_recording: null, save_session: "/tmp/mock-session.json", load_kv: null, save_kv: null,
});
async function openNeuro(page: Page, theme: "light" | "dark", opts: { sample: boolean; connected: boolean }) {
  if (opts.sample) await page.addInitScript(() => localStorage.setItem("nexus.sampleAdapters", "1"));
  await openApp(page, { theme, reduceMotion: true, data: museData(opts.connected) });
  await navButton(page, "Mind–Body").click();
  const field = activeField(page, "Mind–Body");
  await field.getByRole("radio", { name: "Neuro", exact: true }).click();
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Neuro");
  await expect(field.getByRole("heading", { level: 2, name: "Hemi-Sync" })).toBeVisible();
  return field;
}

for (const [name, opts] of [
  ["unwired, Muse connected", { sample: false, connected: true }],
  ["unwired, no Muse", { sample: false, connected: false }],
  ["sample, Muse connected", { sample: true, connected: true }],
  ["sample, no Muse", { sample: true, connected: false }],
] as const) {
  test(`(1112n) Neuro ${name}: sections present, honest tags, axe serious/critical = 0`, async ({ page, theme }) => {
    const field = await openNeuro(page, theme, opts);
    await expect(field.getByRole("heading", { level: 2, name: "Done with the Muse" })).toBeVisible();
    await expect(field.getByRole("heading", { level: 2, name: "Focus and calm over time" })).toBeVisible();
    if (opts.sample) {
      await expect(field.getByText("5 tracks across 3 waves", { exact: false })).toBeVisible();
      await expect(field.getByText("Sample data").first()).toBeVisible();
      await expect(field.getByRole("table", { name: "Tracks done with the Muse" })).toBeVisible();
      await expect(field.getByText("2 of 5 done")).toBeVisible();
      await expect(field.getByRole("region", { name: "Wave 5 Exploring checklist" })).toBeVisible();
    } else {
      await expect(field.getByText("Your Hemi-Sync library isn't connected yet.")).toBeVisible();
      await expect(field.getByText("Needs wiring").first()).toBeVisible();
      await expect(field.getByText("Sample data")).toHaveCount(0);
    }
    await page.waitForTimeout(300);
    expect(await blocking(page)).toEqual([]);
  });
}

test("(1112n) sample library: tracks grouped by wave; with no Muse, Start explains and plays nothing", async ({ page, theme }) => {
  const field = await openNeuro(page, theme, { sample: true, connected: false });
  // His most recent session is in wave 5, so that wave is open and wave 1 is not.
  await expect(field.getByRole("button", { name: /Intro to Focus 15/ })).toBeVisible();
  await expect(field.getByRole("button", { name: /Orientation/ })).toBeHidden();
  await field.getByRole("button", { name: /Intro to Focus 15/ }).click();
  await expect(field.getByText("Intro to Focus 15", { exact: true }).first()).toBeVisible();
  // aria-disabled (not disabled), so it stays clickable and can explain itself; Playwright treats aria-disabled as disabled.
  await expect(field.getByRole("button", { name: "Start" })).toHaveAttribute("aria-disabled", "true");
  await field.getByRole("button", { name: "Start" }).click({ force: true });
  await expect(field.getByText(/Connect your Muse first/).first()).toBeVisible();
  expect(await page.evaluate(() => document.querySelectorAll("audio").length)).toBe(0);
  expect(await blocking(page)).toEqual([]);
});

test("(1112n) Import in unwired mode says it needs wiring and changes nothing", async ({ page, theme }) => {
  const field = await openNeuro(page, theme, { sample: false, connected: true });
  await field.getByRole("button", { name: "Import your collection" }).click();
  await expect(field.getByText("Importing isn't connected yet.")).toBeVisible();
});
