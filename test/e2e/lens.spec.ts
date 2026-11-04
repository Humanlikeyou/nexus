import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";
import type { Page } from "@playwright/test";

/** Amara speaking lens (spec-amara-speaking-hal.md). Events are mocked; Rust owns the real emit. LENS_SHOTS=<dir> saves screenshots. */
const LEVEL = "amara-voice://level";
const STATE = "amara-voice://state";
/** Emits with `ts` from the page clock (the fixture installs a fake clock). */
const emitNow = (page: Page, event: string, payload: Record<string, unknown>) =>
  page.evaluate(([e, p]) => (window as unknown as { __E2E_EMIT: (e: unknown, p: unknown) => void }).__E2E_EMIT(e, { ts: Date.now(), ...p }), [event, payload] as const);
const state = (page: Page, s: string, extra: Record<string, unknown> = {}) => emitNow(page, STATE, { state: s, ...extra });
const level = (page: Page, rms: number) => emitNow(page, LEVEL, { rms, peak: rms, state: "speaking", utteranceId: "u1" });

async function openVoice(page: Page, theme: string, reducedMotion?: "reduce") {
  if (reducedMotion) await page.emulateMedia({ reducedMotion });
  await openApp(page, { theme: theme as never });
  await navButton(page, "Chat & Agents").click();
  const field = activeField(page, "Chat & Agents");
  await field.getByRole("radio", { name: "Voice" }).click();
  const lens = field.locator('.amara-lens[data-size="160"]');
  await expect(lens).toHaveAttribute("data-state", "idle");
  return { field, lens };
}

test("Lens: idle → listening → thinking → speaking → error, labels, live region, scale bound", async ({ page, theme, consoleErrors }) => {
  const { field, lens } = await openVoice(page, theme);
  const shot = async (name: string) => {
    if (process.env.LENS_SHOTS) await field.locator(".voice__lens").screenshot({ path: `${process.env.LENS_SHOTS}/lens-${name}-${theme}.png` });
  };
  await expect(lens).toHaveAttribute("role", "img");
  await shot("idle");
  const region = page.getByTestId("amara-voice-announcer");
  for (const [s, label] of [["listening", "Amara is listening"], ["thinking", "Amara is thinking"]] as const) {
    await state(page, s);
    await expect(lens).toHaveAttribute("data-state", s);
    await expect(lens).toHaveAttribute("aria-label", label);
    await expect(region).toHaveText(label);
    await page.waitForTimeout(s === "thinking" ? 1300 : 0);
    await shot(s);
  }
  await state(page, "speaking", { utteranceId: "u1" });
  for (let i = 0; i < 12; i++) { await level(page, 1); await page.waitForTimeout(33); }
  await expect(lens).toHaveAttribute("aria-label", "Amara is speaking");
  const scale = Number(await lens.evaluate((el) => (el as HTMLElement).style.getPropertyValue("--lens-scale")));
  expect(scale).toBeGreaterThan(1.05);
  expect(scale).toBeLessThanOrEqual(1.12);
  await shot("speaking");
  // Chat header shows the 28px lens only while she talks.
  for (let i = 0; i < 3; i++) await level(page, 0);
  await expect.poll(async () => Number(await lens.evaluate((el) => (el as HTMLElement).style.getPropertyValue("--lens-scale")))).toBeLessThan(1.01);
  await state(page, "error", { reason: "tts_failed" });
  await expect(lens).toHaveAttribute("aria-label", "Amara can't speak right now");
  await expect(field.getByText("Amara can't speak right now. Check Amara Voice.")).toBeVisible();
  await page.waitForTimeout(300);
  await shot("error");
  expect(consoleErrors).toEqual([]);
});

test("Lens: 2s of silence returns to idle; header lens only while speaking", async ({ page, theme }) => {
  const { lens } = await openVoice(page, theme);
  await page.getByRole("radio", { name: "Chat" }).click();
  const header = page.locator(".chat-screen__header .amara-lens");
  await expect(header).toHaveCount(0);
  await state(page, "speaking");
  await level(page, 0.5);
  await expect(header).toHaveCount(1);
  await expect(header).toHaveAttribute("data-size", "28");
  await page.waitForTimeout(2300);
  await expect(header).toHaveCount(0);
  await page.getByRole("radio", { name: "Voice" }).click();
  await expect(lens).toHaveAttribute("data-state", "idle");
});

test("Lens: reduced motion runs no animations and keeps scale at 1", async ({ page, theme }) => {
  const { lens } = await openVoice(page, theme, "reduce");
  await state(page, "thinking");
  await expect(lens).toHaveAttribute("data-state", "thinking");
  expect(await lens.evaluate((el) => el.getAnimations({ subtree: true }).length)).toBe(0);
  await state(page, "speaking");
  await level(page, 1);
  const t = await lens.locator(".amara-lens__core").evaluate((el) => getComputedStyle(el).transform);
  expect(["none", "matrix(1, 0, 0, 1, 0, 0)"]).toContain(t);
});
