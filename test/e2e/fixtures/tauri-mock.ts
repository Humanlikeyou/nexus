/**
 * Mocks Tauri IPC (window.__TAURI_INTERNALS__.invoke → canned responses) so the
 * built console runs in plain Chromium. Commands listed in `failing` reject,
 * which loadSystemSnapshot() turns into data.unavailableSources entries.
 * Unmocked commands are recorded in window.__E2E_UNMOCKED.
 */
import { test as base, expect, type Page } from "@playwright/test";
import { NOW, responses, type Responses } from "./snapshot";

export type Theme = "light" | "dark";

type Reduce = "reduce" | "no-preference";

/**
 * Sets theme, reduced transparency and reduced motion. Chromium goes through CDP, because
 * Playwright's emulateMedia has no reducedTransparency option. WebKit (the real engine) has no
 * prefers-reduced-transparency media feature at all, so there the app's own switch,
 * :root[data-reduce-transparency], stands in for it.
 */
export async function emulateDisplay(page: Page, theme: Theme, rt: Reduce, rm: Reduce = "no-preference") {
  if (page.context().browser()?.browserType().name() === "chromium") {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setEmulatedMedia", {
      features: [
        { name: "prefers-reduced-transparency", value: rt },
        { name: "prefers-reduced-motion", value: rm },
        { name: "prefers-color-scheme", value: theme },
      ],
    });
    return;
  }
  await page.emulateMedia({ colorScheme: theme, reducedMotion: rm });
  // Init scripts run before <html> exists in WebKit.
  const apply = (on: boolean) => {
    const set = () => { if (on) document.documentElement.setAttribute("data-reduce-transparency", "true"); else document.documentElement.removeAttribute("data-reduce-transparency"); };
    if (document.documentElement) set();
    else document.addEventListener("readystatechange", set, { once: true });
  };
  await page.addInitScript(apply, rt === "reduce");
  await page.evaluate(apply, rt === "reduce");
}

/**
 * The app's own reduced-transparency state: the sidebar drops its blur and paints the solid glass
 * fill (--glass-solid). This reads what the app's CSS actually computed, so it is false if the app's
 * fallback breaks, whatever the fixture set up.
 */
export const appFallbackApplied = (page: Page) =>
  page.evaluate(() => {
    const nav = document.querySelector("main.command-surface > nav.system-constellation");
    if (!nav) return false;
    const style = getComputedStyle(nav);
    const probe = document.createElement("i");
    probe.style.background = "var(--glass-solid)";
    document.body.appendChild(probe);
    const solid = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return (style.backdropFilter || "none") === "none" && solid !== "rgba(0, 0, 0, 0)" && style.backgroundColor === solid;
  });

/** Whether reduced transparency took effect in the engine AND the app's fallback is really applied (appFallbackApplied). */
export const reducedTransparency = async (page: Page) =>
  (await page.evaluate(() => matchMedia("(prefers-reduced-transparency: reduce)").matches || document.documentElement.getAttribute("data-reduce-transparency") === "true")) && (await appFallbackApplied(page));

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
  /** The live canned responses, so a spec can change the answer at an exact moment (see setMock). */
  w.__E2E_DATA = data;
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
        if (tool in canned.__byTool) {
          const answer = canned.__byTool[tool] as { __byKey?: Record<string, unknown> };
          // __byKey: a different metric_series per metric key.
          const key = String((args?.args as { key?: unknown } | undefined)?.key ?? "");
          return structuredClone(answer && typeof answer === "object" && answer.__byKey ? answer.__byKey[key] : answer);
        }
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

/** Open the app, then its dashboard: launch lands on the lens, and shell chrome starts at Today. */
export async function openToday(page: Page, options: MockOptions): Promise<void> {
  await openApp(page, options);
  await navButton(page, "Today").click();
  await expect(activeField(page, "Today").locator(".home__summary")).toBeVisible();
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

/** Changes one canned response from now on. Use it for what the backend starts saying mid-test. */
export const setMock = (page: Page, cmd: string, value: unknown) =>
  page.evaluate(([c, v]) => { (window as unknown as { __E2E_DATA: Record<string, unknown> }).__E2E_DATA[c as string] = v; }, [cmd, value] as const);

/** Sidebar button for a section, e.g. "Agents". */
export const navButton = (page: Page, label: string) =>
  page.getByRole("navigation", { name: "Amara OS navigation" }).getByRole("button", { name: new RegExp(`^${label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`) }).first();

/** The visible screen region ("<label> system"). */
export const activeField = (page: Page, label: string) => page.getByRole("region", { name: `${label} system` });

const SUB_VIEW_HOSTS: Record<string, string> = { Fitness: "Mind–Body", Neuro: "Mind–Body", "Neuro history": "Mind–Body", Crews: "Agents", Money: "Business", Ventures: "Business", Work: "Business", Revenue: "Business" };
/** design-1110b B: folded screens that are now a section of a host page (label → host, anchor id). */
const SECTION_HOSTS: Record<string, [string, string]> = { Activity: ["Agents", "activity"], Lists: ["Today", "lists"], Voice: ["Settings", "voice"] };

/** Goes to Today from wherever the page is: from the landing its button, from anywhere else Home first. */
export async function gotoToday(page: Page) {
  const today = navButton(page, "Today");
  if (!(await today.isVisible().catch(() => false))) { await navButton(page, "Home").click(); await routeSettled(page).catch(() => undefined); }
  await navButton(page, "Today").click();
  return activeField(page, "Today");
}

/** The Home chat under the lens (design-1110b A). Opens Home first when it isn't showing. */
export async function openHomeChat(page: Page, { expand = false }: { expand?: boolean } = {}) {
  const panel = page.locator(".landing .hchat");
  if (!(await panel.isVisible().catch(() => false))) { await navButton(page, "Home").click(); await routeSettled(page).catch(() => undefined); }
  await expect(panel).toBeVisible();
  // Expand is the largest the chat gets (no Full page): the whole thread, scrollable.
  if (expand && (await panel.getAttribute("data-state")) !== "expanded") await panel.getByRole("button", { name: "Expand" }).click();
  return panel;
}

/** Resolves once a route transition and its enter animation have finished. */
export const routeSettled = (page: Page) => page.waitForFunction(() => !document.documentElement.dataset.routeMotion);

/** Opens the page's quiet disclosure, the way a person would. */
export async function openMore(page: Page) {
  // A slow chunk can hold the route back, and an error state may have no disclosure at all.
  await page.locator(".active-field details.quiet-more").first().waitFor({ state: "attached", timeout: 2_000 }).catch(() => undefined);
  await routeSettled(page);
  const closed = page.locator(".active-field details.quiet-more:not([open]) > summary");
  while (await closed.count()) await closed.first().click();
}

/** Opens a section, or a sub-view through its host, and returns the screen region. */
export async function openView(page: Page, label: string) {
  const section = SECTION_HOSTS[label];
  if (section) {
    const [host, id] = section;
    const field = host === "Today" ? await gotoToday(page) : (await navButton(page, host).click(), activeField(page, host));
    await field.locator(`#${id}.page-section`).scrollIntoViewIfNeeded();
    return field;
  }
  const host = SUB_VIEW_HOSTS[label];
  await navButton(page, host ?? label).click();
  const field = activeField(page, host ?? label);
  if (host) {
    const radio = field.getByRole("radio", { name: label, exact: true });
    await radio.click();
    await expect(radio).toHaveAttribute("aria-checked", "true");
  }
  return field;
}
