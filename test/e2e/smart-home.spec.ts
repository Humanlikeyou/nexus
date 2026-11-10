import fs from "node:fs";
import { activeField, emitTauriEvent, expect, navButton, openApp, test, openMore, openHomeChat } from "./fixtures/tauri-mock";
import { HA_ENTITIES, responses } from "./fixtures/snapshot";

/** Smart home (Home Assistant) + chat pre-route, all ha_* commands mocked. HA_SHOTS=<dir> saves screenshots. */
const SHOTS = process.env.HA_SHOTS;
const shot = async (page: import("@playwright/test").Page, name: string) => {
  if (!SHOTS) return;
  fs.mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: true });
};

test.describe("Smart home", () => {
  test("devices, toggles, live updates and container badge", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme });
    await navButton(page, "Smart home").click();
    const f = activeField(page, "Smart home");
    await expect(f.getByRole("heading", { level: 1 })).toHaveText("Smart home");
    await openMore(page);
    await expect(f.getByLabel("Home Assistant container: Up")).toBeVisible();
    const lamp = f.getByRole("switch", { name: "Desk lamp" });
    await expect(lamp).toHaveAttribute("aria-checked", "true");
    await expect(f.getByText("21.5 °C")).toBeVisible();
    await expect(f.getByRole("button", { name: "Start" })).toBeDisabled();
    await emitTauriEvent(page, "ha://live_status", { state: "connected" });
    await expect(f.getByLabel("Live updates: Live")).toBeVisible();
    await emitTauriEvent(page, "ha://state_changed", { entityId: "switch.kettle", newState: { ...HA_ENTITIES[1], state: "on" }, oldState: HA_ENTITIES[1], timeFired: null });
    await expect(f.getByRole("switch", { name: "Kettle" })).toHaveAttribute("aria-checked", "true");
    await lamp.click();
    await expect(lamp).toHaveAttribute("aria-checked", "false");
    expect(consoleErrors).toEqual([]);
    await shot(page, `devices-${theme}`);
  });

  test("empty state", async ({ page, theme }) => {
    await openApp(page, { theme, data: { ...responses(), ha_states: [] } });
    await navButton(page, "Smart home").click();
    await expect(activeField(page, "Smart home").getByText("No devices yet.")).toBeVisible();
    await shot(page, `empty-${theme}`);
  });

  test("not configured and container down: setup steps, Start enabled", async ({ page, theme }) => {
    const data = { ...responses(), ha_states: { __reject: "Home Assistant is not configured (check HA_URL and HA_TOKEN)." }, ha_container_status: { running: false, status: "Exited (0) 2 hours ago" } };
    await openApp(page, { theme, data });
    await navButton(page, "Smart home").click();
    const f = activeField(page, "Smart home");
    const inactive = f.locator(".subsystem-inactive");
    await expect(inactive).toContainText("Home Assistant isn't set up");
    await openMore(page);
    await expect(f.getByLabel("Home Assistant container: Down")).toBeVisible();
    await expect(f.getByRole("button", { name: "Start" })).toBeEnabled();
    await shot(page, `inactive-${theme}`);
  });

  test("chat pre-route: HA match shows a Home badge", async ({ page, theme }) => {
    await openApp(page, { theme, data: { ...responses(), ha_conversation: { kind: "match", speech: "Turned on the desk lamp." } } });
    const field = await openHomeChat(page);
    await field.getByLabel("Message Amara").fill("turn on the desk lamp");
    await field.getByLabel("Message Amara").press("Enter");
    const list = field.getByRole("list", { name: "Messages" });
    await expect(list.getByText("Turned on the desk lamp.")).toBeVisible();
    await expect(list.getByLabel("Answered by Home Assistant")).toHaveText("Home");
    await shot(page, `chat-home-${theme}`);
  });
});
