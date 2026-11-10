import { activeField, expect, navButton, openApp, test, openView, openHomeChat } from "./fixtures/tauri-mock";
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
  const field = (await openView(page, "Voice")).locator("#voice");
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
  await state(page, "error", { reason: "unavailable" }); // amara-voice is down
  await expect(lens).toHaveAttribute("aria-label", "Amara can't speak right now");
  await expect(field.getByText("Amara can't speak right now. Check Amara Voice.")).toBeVisible();
  await page.waitForTimeout(300);
  await shot("error");
  expect(consoleErrors).toEqual([]);
});

const scaleOf = (lens: import("@playwright/test").Locator) => lens.evaluate((el) => Number((el as HTMLElement).style.getPropertyValue("--lens-scale")));

test("Lens: 2s without levels decays the meter but keeps speaking; ~12s without state falls back to idle", async ({ page, theme }) => {
  const { lens } = await openVoice(page, theme);
  // design-1110b A: on the Home chat, the reply being spoken is marked while Amara speaks.
  const chat = await openHomeChat(page, { expand: true });
  const header = chat.locator(".hchat__head .amara-lens");
  const spoken = chat.locator("li.chat-message[data-speaking]");
  await expect(header).toHaveAttribute("data-size", "44");
  await expect(spoken).toHaveCount(0);
  await state(page, "speaking");
  for (let i = 0; i < 6; i++) { await level(page, 1); await page.waitForTimeout(33); }
  await expect(spoken).toHaveCount(1);
  await expect(header).toHaveAttribute("data-state", "speaking");
  await page.waitForTimeout(2300);
  await expect.poll(() => scaleOf(header)).toBeLessThan(1.01);
  await expect(spoken).toHaveCount(1); // state holds between heartbeats
  await page.clock.fastForward(12_500); // missed heartbeats
  await expect(spoken).toHaveCount(0);
  await openView(page, "Voice");
  await expect(lens).toHaveAttribute("data-state", "idle");
});

test("Lens: idle with model_loading shows a loading note; bridge starts with the Voice panel and stops after", async ({ page, theme }) => {
  const { field, lens } = await openVoice(page, theme);
  const calls = () => page.evaluate(() => ((window as unknown as { __E2E_UNMOCKED: string[] }).__E2E_UNMOCKED).filter((c) => c.startsWith("voice_bridge_")));
  // The landing lens runs the bridge from launch; leaving it stops it, then the Voice panel starts it.
  const opened = ["voice_bridge_start", "voice_bridge_stop", "voice_bridge_start"];
  await expect.poll(calls).toEqual(opened);
  await state(page, "idle", { reason: "model_loading" });
  await expect(lens).toHaveAttribute("data-state", "idle");
  await expect(field.getByText("Loading voice model…")).toBeVisible();
  await state(page, "listening");
  await expect(field.getByText("Loading voice model…")).toHaveCount(0);
  await navButton(page, "Agents").click();
  await expect.poll(calls).toEqual([...opened, "voice_bridge_stop"]);
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

test.describe("Lens: motion means she's alive", () => {
  const hero = (page: Page) => page.locator('.landing .amara-lens[data-size="320"]');
  const names = (lens: import("@playwright/test").Locator) => lens.evaluate((el) => el.getAnimations({ subtree: true }).map((a) => (a as CSSAnimation).animationName).sort());

  test("listening breathes and drifts, on periods that don't line up, and never scales", async ({ page, theme }) => {
    await openApp(page, { theme });
    await expect(hero(page)).toHaveAttribute("data-state", "listening");
    const running = await names(hero(page));
    expect(running.filter((name) => name === "amara-lens-breath").length).toBeGreaterThanOrEqual(3);
    expect(running).toContain("amara-lens-drift");
    const periods = await hero(page).evaluate((el) => [...new Set(el.getAnimations({ subtree: true }).map((a) => a.effect?.getTiming().duration))]);
    expect(periods.length).toBeGreaterThanOrEqual(4);
    const keyframes = await hero(page).evaluate((el) => el.getAnimations({ subtree: true }).flatMap((a) => (a.effect as KeyframeEffect).getKeyframes()).map((f) => String(f.transform ?? "")));
    expect(keyframes.join()).not.toContain("scale");
  });

  test("thinking orbits the ring and leaves the iris still", async ({ page, theme }) => {
    await openApp(page, { theme });
    await emitNow(page, STATE, { state: "thinking" });
    await expect(hero(page)).toHaveAttribute("data-state", "thinking");
    const running = await names(hero(page));
    expect(running).toContain("amara-lens-orbit");
    expect(running).not.toContain("amara-lens-breath");
    expect(running).not.toContain("amara-lens-drift");
  });

  test("speaking with no level data doesn't move", async ({ page, theme }) => {
    await openApp(page, { theme });
    await emitNow(page, STATE, { state: "speaking" });
    await expect(hero(page)).toHaveAttribute("data-state", "speaking");
    await page.waitForTimeout(300);
    expect(await names(hero(page))).toEqual([]);
    expect(await hero(page).evaluate((el) => Number((el as HTMLElement).style.getPropertyValue("--lens-level")))).toBe(0);
  });

  test("error is still", async ({ page, theme }) => {
    await openApp(page, { theme });
    await emitNow(page, STATE, { state: "error" });
    await expect(hero(page)).toHaveAttribute("data-state", "error");
    expect(await names(hero(page))).toEqual([]);
  });

  test("reduced motion: speaking changes brightness only", async ({ page, theme }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await openApp(page, { theme });
    await emitNow(page, STATE, { state: "speaking" });
    await expect(hero(page)).toHaveAttribute("data-state", "speaking");
    const core = hero(page).locator(".amara-lens__core");
    const opacity = () => core.evaluate((el) => Number(getComputedStyle(el).opacity));
    const quiet = await opacity();
    for (let i = 0; i < 15; i++) { await emitNow(page, LEVEL, { rms: .05, peak: .1, state: "speaking" }); await page.waitForTimeout(33); }
    await expect.poll(opacity).toBeGreaterThan(quiet + .08);
    expect(await core.evaluate((el) => getComputedStyle(el).transform)).toBe("none");
    expect(await hero(page).evaluate((el) => (el as HTMLElement).style.getPropertyValue("--lens-scale"))).toBe("1");
  });

  test("a hidden window pauses the light", async ({ page, theme }) => {
    await openApp(page, { theme });
    await expect(hero(page)).toHaveAttribute("data-state", "listening");
    await page.evaluate(() => {
      Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await expect(hero(page)).toHaveAttribute("data-paused", "");
    expect(await hero(page).evaluate((el) => el.getAnimations({ subtree: true }).every((a) => a.playState === "paused"))).toBe(true);
  });
});
