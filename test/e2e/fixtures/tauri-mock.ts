/**
 * Mocks Tauri IPC (window.__TAURI_INTERNALS__.invoke → canned responses) so the
 * built console runs in plain Chromium. Commands listed in `failing` reject,
 * which loadSystemSnapshot() turns into data.unavailableSources entries.
 * Unmocked commands are recorded in window.__E2E_UNMOCKED.
 */
import { test as base, expect, type Page } from "@playwright/test";
import { NOW, responses, type Responses } from "./snapshot";

export type Theme = "light" | "dark";

export interface MockOptions {
  theme: Theme;
  reduceMotion?: boolean;
  failing?: string[];
  data?: Responses;
  /** Per-command response delay in ms (e.g. to make the boot splash appear). */
  delays?: Record<string, number>;
}

type InitArgs = { data: Responses; theme: Theme; reduceMotion: boolean; failing: string[]; delays: Record<string, number> };

function initScript({ data, theme, reduceMotion, failing, delays }: InitArgs) {
  localStorage.setItem("nexus.appearance.theme", theme);
  localStorage.setItem("nexus.appearance.reduceMotion", String(reduceMotion));
  localStorage.setItem("nexus.sidebarWidth", "220");
  const callbacks = new Map<number, unknown>();
  let next = 1;
  const w = window as unknown as Record<string, unknown>;
  const unmocked: string[] = [];
  w.__E2E_UNMOCKED = unmocked;
  /** In-page record of invoke calls (args included) for specs that assert what was sent. */
  const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
  w.__E2E_CALLS = calls;
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  const listeners: Array<{ event: string; handler: number }> = [];
  /** e2e: deliver a Tauri event to every listener registered for it. */
  w.__E2E_EMIT = (event: string, payload: unknown) => {
    for (const l of listeners) if (l.event === event) (callbacks.get(l.handler) as ((e: unknown) => void) | undefined)?.({ event, id: l.handler, payload });
  };
  w.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback: (cb: unknown) => { const id = next++; callbacks.set(id, cb); return id; },
    unregisterCallback: (id: number) => callbacks.delete(id),
    convertFileSrc: (p: string) => p,
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      if (delays[cmd]) await new Promise((r) => setTimeout(r, delays[cmd]));
      if (!cmd.startsWith("plugin:")) calls.push({ cmd, args: args ? structuredClone(args) : undefined });
      if (failing.includes(cmd)) throw new Error(`e2e: ${cmd} failed`);
      let canned = data[cmd] as { __byTool?: Record<string, unknown>; __reject?: unknown; __sequence?: unknown[] } | undefined;
      // __sequence: one canned answer per call (the last one repeats); entries may be { __reject }.
      if (canned && typeof canned === "object" && Array.isArray(canned.__sequence)) {
        const seq = canned.__sequence;
        canned = (seq.length > 1 ? seq.shift() : seq[0]) as typeof canned;
        if (!(canned && typeof canned === "object" && "__reject" in canned)) return structuredClone(canned);
      }
      if (canned && typeof canned === "object" && "__reject" in canned) throw structuredClone(canned.__reject);
      if (canned && typeof canned === "object" && canned.__byTool) {
        const tool = String(args?.tool ?? "");
        if (tool in canned.__byTool) return structuredClone(canned.__byTool[tool]);
        throw { kind: "tool_not_allowed", detail: tool };
      }
      if (cmd in data) return structuredClone(data[cmd]);
      if (cmd === "plugin:event|listen") { listeners.push({ event: String(args?.event), handler: Number(args?.handler) }); return next++; }
      if (cmd.startsWith("plugin:event|")) return next++;
      unmocked.push(cmd);
      if (cmd.startsWith("list_") || cmd.startsWith("read_")) return [];
      throw new Error(`e2e mock: ${cmd} not mocked`);
    },
  };
}

/** Install the mock and a running clock at NOW, then open the app. */
export async function openApp(page: Page, options: MockOptions): Promise<void> {
  await page.clock.install({ time: new Date(NOW) });
  // A frozen Date.now() would make every snapshot look like INITIAL_SNAPSHOT (= loading).
  await page.clock.resume();
  await page.addInitScript(initScript, {
    data: options.data ?? responses(),
    theme: options.theme,
    reduceMotion: options.reduceMotion ?? false,
    failing: options.failing ?? [],
    delays: options.delays ?? {},
  });
  await page.goto("/");
  await expect(page.getByRole("navigation", { name: "Amara OS navigation" })).toBeVisible();
}

/** Expected noise: loadSystemSnapshot() warns (not errors) for failing reads. */
const IGNORED_CONSOLE = [/favicon/i, /404 \(Not Found\)/];

type Fixtures = { theme: Theme; consoleErrors: string[] };

/** `theme` comes from the project (light/dark); `consoleErrors` collects console errors + page errors. */
export const test = base.extend<Fixtures>({
  theme: ["light", { option: true }],
  consoleErrors: async ({ page }, use) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    page.on("console", (m) => {
      if (m.type() === "error" && !IGNORED_CONSOLE.some((re) => re.test(m.text()))) errors.push(m.text());
    });
    await use(errors);
  },
});

export { expect };

/** Emits a Tauri event inside the page (see __E2E_EMIT). */
export const emitTauriEvent = (page: Page, event: string, payload: unknown) =>
  page.evaluate(([e, p]) => (window as unknown as { __E2E_EMIT: (e: unknown, p: unknown) => void }).__E2E_EMIT(e, p), [event, payload] as const);

/** Sidebar button for a section, e.g. "Chat & Agents". */
export const navButton = (page: Page, label: string) =>
  page.getByRole("navigation", { name: "Amara OS navigation" }).getByRole("button", { name: new RegExp(`^${label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`) }).first();

/** The visible screen region ("<label> system"). */
export const activeField = (page: Page, label: string) => page.getByRole("region", { name: `${label} system` });
