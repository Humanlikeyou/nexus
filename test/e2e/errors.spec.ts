import { activeField, expect, openApp, openView, test, openMore } from "./fixtures/tauri-mock";

/** (e) One scenario per screen: its command fails → it lands in unavailableSources → the screen's error state. */
const SCENARIOS: { label: string; failing: string; check: (field: ReturnType<typeof activeField>) => Promise<void> }[] = [
  { label: "Today", failing: "get_terrence_runtime_health", check: async (f) => { await expect(f.getByText("Amara runtime unavailable")).toBeVisible(); await expect(f.getByRole("button", { name: "Retry" }).first()).toBeVisible(); } },
  { label: "Agents", failing: "get_terrence_runtime_health", check: async (f) => { await expect(f.getByText("Status unavailable")).toBeVisible(); } },
  { label: "Fitness", failing: "get_noop_health_snapshot", check: async (f) => { await expect(f.getByText("Health sync is unavailable")).toBeVisible(); await expect(f.getByRole("button", { name: "Retry" }).first()).toBeVisible(); } },
  { label: "Neuro", failing: "get_muse_state", check: async (f) => { await expect(f.getByText("Unavailable", { exact: true }).first()).toBeVisible(); } },
  { label: "Tasks", failing: "read_tasks", check: async (f) => { await expect(f.getByText("Task source unavailable")).toBeVisible(); } },
  { label: "Enterprise", failing: "read_tasks", check: async (f) => { await expect(f.getByText("Task source unavailable")).toBeVisible(); } },
  { label: "Settings", failing: "get_muse_state", check: async (f) => { await expect(f.getByText("Unavailable", { exact: true }).first()).toBeVisible(); await expect(f.getByRole("button", { name: "Retry" })).toBeVisible(); } },
];

for (const { label, failing, check } of SCENARIOS) {
  test(`(e) ${label}: ${failing} unavailable shows the error state`, async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme, failing: [failing] });
    const field = await openView(page, label);
    await openMore(page); // failures that aren't the page's headline sit with their section
    await expect(field.getByRole("heading", { level: 1 })).toBeVisible();
    await check(field);
    expect(consoleErrors).toEqual([]);
  });
}
