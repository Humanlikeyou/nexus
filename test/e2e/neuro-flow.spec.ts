import type { Page } from "@playwright/test";
import { activeField, expect, navButton, openApp, openMore, setMock, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** Neurofeedback end to end, on mocked EEG only. NEURO_SHOTS=<dir> saves screenshots. */
const SHOTS = process.env.NEURO_SHOTS;
const shot = async (page: Page, name: string) => { if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}.png` }); };

const museData = () => ({
  ...responses(),
  get_muse_state: { connected: true, device_name: "Muse 2", streaming: false },
  start_eeg_recording: "/tmp/mock-recording.bin", start_eeg_stream: null, stop_eeg_stream: null, stop_eeg_recording: null, save_session: "/tmp/mock-session.json",
  load_kv: null, save_kv: null,
});

/** A mock headset: 256 Hz in packets of 12, on the page's clock. `alpha` sets how strong it is. */
async function startHeadset(page: Page) {
  await page.evaluate(() => {
    const w = window as unknown as { __E2E_EMIT: (e: string, p: unknown) => void; __eeg: { alpha: number; silent: string[]; blink: number; aux: boolean }; __n: number };
    w.__eeg = { alpha: 14, silent: [], blink: 0, aux: false };
    w.__n = 0;
    setInterval(() => {
      for (let packet = 0; packet < 2; packet++) {
        const start = w.__n; w.__n += 12;
        const samples = (seed: number) => Array.from({ length: 12 }, (_, i) => {
          const t = (start + i) / 256;
          return w.__eeg.alpha * Math.sin(2 * Math.PI * 10 * t) + 4 * Math.sin(2 * Math.PI * 22 * t + seed) + 3 * Math.sin(2 * Math.PI * 6 * t) + 2 * Math.sin(2 * Math.PI * 47 * t + seed);
        });
        for (const [i, channel] of ["TP9", "AF7", "AF8", "TP10"].entries()) {
          if (w.__eeg.silent.includes(channel)) continue;
          const frontal = channel === "AF7" || channel === "AF8";
          const values = samples(i).map((v, k) => (frontal && w.__eeg.blink > 0 ? v + 260 * Math.sin(Math.PI * ((start + k) % 40) / 40) : v));
          w.__E2E_EMIT("muse-eeg", { timestamp: Date.now(), channel, samples: values });
        }
        if (w.__eeg.aux) w.__E2E_EMIT("muse-eeg", { timestamp: Date.now(), channel: "AUX", samples: samples(9) });
      }
      if (w.__eeg.blink > 0) w.__eeg.blink -= 1;
    }, 80);
  });
}
const setHeadset = (page: Page, patch: Record<string, unknown>) => page.evaluate((p) => { Object.assign((window as unknown as { __eeg: object }).__eeg, p); }, patch);

async function toFit(page: Page, theme: "light" | "dark") {
  await openApp(page, { theme, data: museData() });
  await navButton(page, "Mind–Body").click();
  const field = activeField(page, "Mind–Body");
  await field.getByRole("radio", { name: "Neuro", exact: true }).click();
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Neuro");
  await expect(field.getByRole("radiogroup", { name: "Goal" })).toBeVisible();
  await shot(page, `neuro-setup-${theme}`);
  await setMock(page, "get_muse_state", { connected: true, device_name: "Muse 2", streaming: true });
  await field.getByRole("button", { name: "Start session" }).click();
  await expect(field.getByRole("list", { name: "Sensor contact" })).toBeVisible();
  await startHeadset(page);
  await page.clock.runFor(4_000);
  return field;
}

test("the whole session: goal, fit, baseline, training, summary", async ({ page, theme, consoleErrors }) => {
  test.setTimeout(60_000);
  const field = await toFit(page, theme);
  await expect(field.getByRole("button", { name: "Begin baseline" })).toBeEnabled();
  await expect(field.getByRole("list", { name: "Sensor contact" }).getByText("Good contact")).toHaveCount(4);
  await shot(page, `neuro-fit-${theme}`);
  await field.getByRole("button", { name: "Begin baseline" }).click();
  await expect(field.getByText("Eyes open, look softly at the light")).toBeVisible();
  await shot(page, `neuro-baseline-${theme}`);
  await page.clock.runFor(21_000);
  await expect(field.getByText("Close your eyes")).toBeVisible();
  await page.clock.runFor(21_000);
  await expect(field.getByRole("region", { name: "Live feedback" })).toBeVisible();

  await setHeadset(page, { alpha: 40 }); // well above his baseline
  await page.clock.runFor(8_000);
  await expect(field.getByRole("status").filter({ hasText: "In your target" })).toBeVisible();
  await page.waitForTimeout(1_800); // real time for the light to rise
  const level = await field.locator(".nf-lens .amara-lens").evaluate((el) => Number((el as HTMLElement).style.getPropertyValue("--lens-level")));
  expect(level).toBeGreaterThan(.4);
  await shot(page, `neuro-training-${theme}`);

  // A dropped sensor holds the light and says why.
  await setHeadset(page, { silent: ["TP10"] });
  await page.clock.runFor(2_500);
  await expect(field.getByText("Feedback held")).toBeVisible();
  await expect(field.getByRole("region", { name: "Live feedback" }).getByText("Holding: the headset signal dropped out.")).toBeVisible();
  await shot(page, `neuro-held-${theme}`);
  await setHeadset(page, { silent: [] });
  await page.clock.runFor(3_000);
  await expect(field.getByText("Feedback held")).toBeHidden();

  await field.getByRole("button", { name: "Stop session" }).click();
  await expect(field.getByRole("region", { name: "Session summary" })).toBeVisible();
  await expect(field.getByText("of clean time in your target")).toBeVisible();
  await shot(page, `neuro-summary-${theme}`);
  await field.getByLabel(/What helped/).fill("Long exhales, soft eyes");
  await field.getByRole("button", { name: "Save note" }).click();
  await expect(field.getByRole("button", { name: "Saved" })).toBeVisible();
  // The long fast-forward also wakes the background scheduler, which has no mocks here.
  expect(consoleErrors.filter((error) => !/^\[(scheduler|continuity)\]/.test(error))).toEqual([]);
});

test("no good contact, no baseline", async ({ page, theme }) => {
  const field = await toFit(page, theme);
  await setHeadset(page, { silent: ["AF7"] });
  await page.clock.runFor(3_000);
  await expect(field.getByRole("button", { name: "Waiting for good contact" })).toBeDisabled();
  await expect(field.getByRole("list", { name: "Sensor contact" }).getByText("No signal")).toHaveCount(1);
});

test("no headset: the page asks for one, and offers no training", async ({ page, theme }) => {
  await openApp(page, { theme, data: { ...responses(), load_kv: null } });
  await navButton(page, "Mind–Body").click();
  const field = activeField(page, "Mind–Body");
  await field.getByRole("radio", { name: "Neuro", exact: true }).click();
  await expect(field.getByRole("button", { name: "Find Muse" })).toBeVisible();
  await expect(field.getByRole("radiogroup", { name: "Goal" })).toHaveCount(0);
});

test("the electrode appears only with real AUX data, and only ever reads", async ({ page, theme }) => {
  const field = await toFit(page, theme);
  await expect(field.getByText(/Myndlift/)).toHaveCount(0);
  await setHeadset(page, { aux: true });
  await page.clock.runFor(1_000);
  await expect(field.getByText("Myndlift electrode: reading, read only")).toBeVisible();
});

test("Today shows the readiness line, and it opens Mind–Body", async ({ page, theme }) => {
  await openApp(page, { theme });
  await navButton(page, "Today").click();
  const today = activeField(page, "Today");
  const line = today.getByRole("button", { name: /^Open Mind–Body\. Readiness/ });
  await expect(line).toBeVisible();
  await line.click();
  await expect(activeField(page, "Mind–Body").getByRole("heading", { level: 1 })).toHaveText("Mind–Body");
  await openMore(page);
});
