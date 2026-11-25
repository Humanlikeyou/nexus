import AxeBuilder from "@axe-core/playwright";
import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** 0045 gallery: the Neuro "Last session" ring is a WebGL radial spectrum when the session has band data, with a table as its text alternative. */
const sess = (i: number, b: number[]) => ({ id: `b${i}`, device_type: "muse", start: `2026-11-${String(10 + i).padStart(2, "0")}T21:00:00+09:00`, end: `2026-11-${String(10 + i).padStart(2, "0")}T21:20:00+09:00`, duration_secs: 1200, data_points: 100, summary: { avg_focus: 0.6, avg_relaxation: 0.6 }, samples: { stepSecs: 5, focus: [50, 60], calm: [60, 62], bands: { delta: [b[0], b[0]], theta: [b[1], b[1]], alpha: [b[2], b[2]], beta: [b[3], b[3]], gamma: [b[4], b[4]] } } });
const SESS = [8, 7, 6, 5].map((i) => sess(i, [18 - i, 14, 20 + i, 24, 13]));

test("Neuro last session: 3D band ring (or its flat fallback) with a text alternative, axe serious/critical = 0", async ({ page, theme }) => {
  await openApp(page, { theme, reduceMotion: true, data: { ...responses(), list_sessions: SESS, get_muse_state: { connected: true, device_name: "Muse 2", streaming: false }, load_kv: null, save_kv: null } });
  await navButton(page, "Mind–Body").click();
  const field = activeField(page, "Mind–Body");
  await field.getByRole("radio", { name: "Neuro", exact: true }).click();
  await expect(field.getByRole("heading", { level: 1 })).toHaveText("Neuro");
  await expect(page.getByTestId("neuro-brain-ring")).toBeVisible();
  const gl = await page.evaluate(() => !!document.createElement("canvas").getContext("webgl2"));
  if (gl) await expect(page.locator(".d18-brain__canvas")).toBeVisible();
  await expect(field.getByRole("table", { name: /Brainwave bands of your last session/ })).toContainText("Alpha");
  await expect(field.getByTestId("neuro-live-ring")).toContainText("Shows while Muse is connected");
  await page.waitForTimeout(500);
  const r = await new AxeBuilder({ page }).analyze();
  expect(r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => v.id)).toEqual([]);
});
