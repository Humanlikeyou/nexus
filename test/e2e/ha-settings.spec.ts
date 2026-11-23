import { activeField, expect, navButton, openApp, openSettingsTab, test, openMore } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

/** Settings › Devices › Home Assistant: write-only token, loopback URL, test connection, Smart home link. */
const TOKEN = "e2e-fake-ha-token-value";

test.describe("Home Assistant settings", () => {
  test("missing commands show Coming soon, not an error", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme });
    await openSettingsTab(page, "Devices");
    await openMore(page);
    const group = activeField(page, "Settings").locator("#home-assistant");
    await expect(group).toContainText("Coming soon: needs Nexus update.");
    await expect(group.getByRole("button", { name: "Test connection" })).toHaveCount(0);
    expect(consoleErrors).toEqual([]);
  });

  test("save URL + token, token never rendered, test connection", async ({ page, theme, consoleErrors }) => {
    const data = { ...responses(), ha_get_config: { url: null, tokenSet: false, tokenSource: null }, ha_set_config: null, ha_test_connection: { ok: true, version: "2026.10.1" } };
    await openApp(page, { theme, data});
    await openSettingsTab(page, "Devices");
    await openMore(page);
    const group = activeField(page, "Settings").locator("#home-assistant");
    await expect(group.getByText("Not set", { exact: true })).toBeVisible();
    const address = group.getByLabel("Address");
    await expect(address).toHaveValue("");
    await expect(address).toHaveAttribute("placeholder", "http://127.0.0.1:8123");
    await address.fill("https://127.0.0.1:8123");
    await address.blur();
    await expect(group).toContainText("Use http://, not https://");
    await address.fill("http://example.com");
    await address.blur();
    await expect(group).toContainText("Use a local address on this Mac");
    await expect(group.getByRole("button", { name: "Save" })).toBeDisabled();
    await address.fill("http://localhost:8123");
    const token = group.getByLabel("Access token", { exact: true });
    await expect(token).toHaveAttribute("type", "password");
    await expect(token).toHaveAttribute("autocomplete", "off");
    await expect(token).toHaveValue("");
    await token.fill(TOKEN);
    await group.getByRole("button", { name: "Save" }).click();
    await expect(group.getByText("Saved.", { exact: true })).toBeVisible();
    await expect(group.getByLabel("Access token", { exact: true })).toHaveValue("");
    expect(await page.content()).not.toContain(TOKEN);
    await group.getByRole("button", { name: "Test connection" }).click();
    await expect(group).toContainText("Connected to Home Assistant 2026.10.1.");
    expect(consoleErrors).toEqual([]);
  });

  test("saved token: Replace/Clear, rejected test copy", async ({ page, theme }) => {
    const data = { ...responses(), ha_get_config: { url: "http://127.0.0.1:8123", tokenSet: true, tokenSource: "app" }, ha_clear_token: { fallbackActive: true }, ha_test_connection: { ok: false, kind: "auth_rejected" } };
    await openApp(page, { theme, data });
    await openSettingsTab(page, "Devices");
    await openMore(page);
    const group = activeField(page, "Settings").locator("#home-assistant");
    await expect(group.getByText("Saved", { exact: true })).toBeVisible();
    await expect(group.getByText("Saved in Nexus")).toBeVisible();
    await expect(group.locator('input[type="password"]')).toHaveCount(0);
    await group.getByRole("button", { name: "Replace token" }).click();
    await expect(group.getByLabel("New access token")).toHaveValue("");
    await group.getByRole("button", { name: "Test connection" }).click();
    await expect(group).toContainText("Home Assistant rejected the token.");
    await group.getByRole("button", { name: "Clear token" }).click();
    await expect(group).toContainText("Token cleared. A token from the environment is still in use.");
  });

  test("Smart home setup state links to the Settings group", async ({ page, theme }) => {
    const data = { ...responses(), ha_states: { __reject: "not_configured: Home Assistant is not configured (check HA_URL and HA_TOKEN)." }, ha_get_config: { url: null, tokenSet: false, tokenSource: null } };
    await openApp(page, { theme, data });
    await navButton(page, "Smart home").click();
    await activeField(page, "Smart home").getByRole("button", { name: "Open Home Assistant settings" }).click();
    const group = activeField(page, "Settings").locator("#home-assistant");
    await expect(group).toBeInViewport();
    await expect(group.getByLabel("Address")).toBeVisible();
  });
  test("Test connection sends the edited fields; bad URL shows described inline error", async ({ page, theme }) => {
    const data = { ...responses(), ha_get_config: { url: "http://127.0.0.1:8123", tokenSet: true, tokenSource: "app" }, ha_test_connection: { ok: true, version: "2026.10.1" } };
    await openApp(page, { theme, data });
    await openSettingsTab(page, "Devices");
    await openMore(page);
    const group = activeField(page, "Settings").locator("#home-assistant");
    const address = group.getByLabel("Address");
    await expect(address).toHaveValue("http://127.0.0.1:8123");
    await address.fill("https://127.0.0.1:8123");
    const err = group.getByText("Use http://, not https://, for Home Assistant on this Mac.");
    await expect(err).toBeVisible();
    const errId = await err.getAttribute("id");
    expect((await address.getAttribute("aria-describedby"))?.split(" ")).toContain(errId);
    await expect(address).toHaveAttribute("aria-invalid", "true");
    await expect(group.getByRole("button", { name: "Test connection" })).toBeDisabled();
    await address.fill("http://localhost:8124");
    await group.getByRole("button", { name: "Replace token" }).click();
    await group.getByLabel("New access token").fill(TOKEN);
    await group.getByRole("button", { name: "Test connection" }).click();
    await expect(group).toContainText("Connected to Home Assistant 2026.10.1.");
    const args = await page.evaluate(() => (window as unknown as { __E2E_CALLS: Array<{ cmd: string; args?: unknown }> }).__E2E_CALLS.filter((c) => c.cmd === "ha_test_connection").map((c) => c.args));
    expect(args).toEqual([{ url: "http://localhost:8124", token: TOKEN }]);
  });
  test("new address needs the token re-entered before Test; token_required copy", async ({ page, theme }) => {
    const data = { ...responses(), ha_get_config: { url: "http://127.0.0.1:8123", tokenSet: true, tokenSource: "app" }, ha_test_connection: { ok: false, kind: "token_required" } };
    await openApp(page, { theme, data });
    await openSettingsTab(page, "Devices");
    await openMore(page);
    const group = activeField(page, "Settings").locator("#home-assistant");
    const address = group.getByLabel("Address");
    await expect(address).toHaveValue("http://127.0.0.1:8123");
    await address.fill("http://localhost:8124");
    const test = group.getByRole("button", { name: "Test connection" });
    await expect(test).toBeDisabled();
    const hint = group.getByText("Re-enter the token to test a new address.");
    await expect(hint).toBeVisible();
    const token = group.getByLabel("New access token");
    expect((await token.getAttribute("aria-describedby"))?.split(" ")).toContain(await hint.getAttribute("id"));
    await token.fill(TOKEN);
    await expect(test).toBeEnabled();
    await test.click();
    await expect(group.getByRole("status").filter({ hasText: "Re-enter the token to test a new address." })).toBeVisible();
  });
});
