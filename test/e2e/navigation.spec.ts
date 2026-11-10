import { activeField, expect, navButton, openApp, openHomeChat, openView, routeSettled, test, openMore } from "./fixtures/tauri-mock";
import { responses } from "./fixtures/snapshot";

const SECTIONS: { label: string; h1: RegExp }[] = [
  { label: "Today", h1: /^Command Center$/ },
  { label: "Agents", h1: /^Agents$/ },
  { label: "Mind–Body", h1: /^Mind–Body$/ },
  { label: "Business", h1: /^Business$/ },
  { label: "Settings", h1: /^Settings$/ },
  { label: "Smart home", h1: /^Smart home$/ },
];

test.describe("sections", () => {
  for (const { label, h1 } of SECTIONS) {
    test(`(a) ${label} renders its h1 with no console errors`, async ({ page, theme, consoleErrors }) => {
      await openApp(page, { theme });
      await navButton(page, label).click();
      await expect(activeField(page, label).getByRole("heading", { level: 1 })).toHaveText(h1);
      expect(consoleErrors).toEqual([]);
    });
  }
});

test.describe("sub-views and anchors", () => {
  type Field = ReturnType<typeof activeField>;
  const SUBVIEWS: [string, string, (f: Field) => Promise<void>][] = [
    ["Agents", "Crews", (f) => expect(f.getByRole("heading", { level: 1 })).toHaveText("Crews")],
    ["Mind–Body", "Fitness", (f) => expect(f.getByRole("heading", { level: 1 })).toHaveText("Fitness")],
    ["Mind–Body", "Neuro", (f) => expect(f.getByRole("heading", { level: 1 })).toHaveText("Neuro")],
    ["Mind–Body", "Neuro history", async (f) => { await expect(f.getByRole("heading", { level: 1 })).toHaveText("Neuro history"); await expect(f.getByRole("table", { name: "Recordings" })).toBeVisible(); }],
    ["Business", "Money", (f) => expect(f.getByRole("heading", { level: 1 })).toHaveText("Business")],
  ];
  for (const [host, view, check] of SUBVIEWS) {
    test(`(b) ${host} › ${view} via the SegmentedControl`, async ({ page, theme, consoleErrors }) => {
      await openApp(page, { theme });
      await navButton(page, host).click();
      const field = activeField(page, host);
      await field.getByRole("radio", { name: view, exact: true }).click();
      await expect(field.getByRole("radio", { name: view, exact: true })).toHaveAttribute("aria-checked", "true");
      await check(field);
      expect(consoleErrors).toEqual([]);
    });
  }

  const ANCHORS: [string, string, string][] = [
    ["Settings", "Settings › Devices", "#devices"],
    ["Settings", "About Amara OS", "#about"],
    ["Today", "Home › Goals", "#goals"],
    // design-1110b B: the folded screens are sections of their new host.
    ["Today", "Today › Lists", "#lists"],
    ["Agents", "Agents › Activity", "#activity"],
    ["Settings", "Settings › Voice", "#voice"],
  ];
  for (const [host, command, anchor] of ANCHORS) {
    test(`(b) ${command} scrolls ${anchor} into view`, async ({ page, theme, consoleErrors }) => {
      await openApp(page, { theme });
      await page.keyboard.press("Meta+k");
      await page.getByRole("combobox", { name: "Search or ask Amara" }).fill(command);
      await page.keyboard.press("Enter");
      const target = activeField(page, host).locator(anchor);
      await expect(target).toBeInViewport();
      expect(consoleErrors).toEqual([]);
    });
  }
});

test.describe("Mind–Body host: nothing folded in is out of reach", () => {
  const selected = (page: import("@playwright/test").Page, name: string) => activeField(page, "Mind–Body").getByRole("radio", { name, exact: true });

  test("the sidebar has six items and Overview is the default", async ({ page, theme }) => {
    await openApp(page, { theme });
    await navButton(page, "Mind–Body").click();
    await expect(page.locator("nav.system-constellation > .nav-group button")).toHaveCount(6);
    await expect(page.locator("nav.system-constellation").getByRole("button", { name: /^(Fitness|Neuro)/ })).toHaveCount(0);
    await expect(activeField(page, "Mind–Body").getByRole("radiogroup", { name: "Mind–Body view" }).getByRole("radio")).toHaveText(["Overview", "Fitness", "Neuro", "Neuro history"]);
    await expect(selected(page, "Overview")).toHaveAttribute("aria-checked", "true");
    await expect(activeField(page, "Mind–Body").getByRole("heading", { level: 1 })).toHaveText("Mind–Body");
  });

  test("Today's links land on the right sub-view", async ({ page, theme, consoleErrors }) => {
    await openApp(page, { theme });
    for (const [link, view, h1] of [["Open Fitness", "Fitness", "Fitness"], ["Open insights in Fitness", "Fitness", "Fitness"], ["Open Neuro", "Neuro", "Neuro"]] as const) {
      await page.keyboard.press("Meta+1"); // back to the lens: Today is a landing button
      await navButton(page, "Today").click();
      await activeField(page, "Today").getByRole("heading", { level: 1 }).waitFor();
      await openMore(page);
      await activeField(page, "Today").getByRole("button", { name: link, exact: true }).click();
      await expect(selected(page, view)).toHaveAttribute("aria-checked", "true");
      await expect(activeField(page, "Mind–Body").getByRole("heading", { level: 1 })).toHaveText(h1);
    }
    expect(consoleErrors).toEqual([]);
  });

  for (const [query, view] of [["Fitness", "Fitness"], ["Mind–Body › Neuro", "Neuro"], ["Neuro history", "Neuro history"]] as const) {
    test(`⌘K "${query}" opens ${view}`, async ({ page, theme }) => {
      await openApp(page, { theme });
      await page.keyboard.press("Meta+k");
      await page.getByRole("combobox", { name: "Search or ask Amara" }).fill(query);
      await page.keyboard.press("Enter");
      await expect(selected(page, view)).toHaveAttribute("aria-checked", "true");
    });
  }

  test("Start session on an empty history opens Neuro", async ({ page, theme }) => {
    await openApp(page, { theme, data: { ...responses(), list_sessions: [] } });
    const field = await openView(page, "Neuro history");
    await field.getByRole("button", { name: "Start session" }).click();
    await expect(selected(page, "Neuro")).toHaveAttribute("aria-checked", "true");
    await expect(field.getByRole("heading", { level: 1 })).toHaveText("Neuro");
  });
});

test("(c) ⌘1–6 open the six sections in order", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme });
  for (const [i, { label, h1 }] of SECTIONS.entries()) {
    await page.keyboard.press(`Meta+${i + 1}`);
    // ⌘1 is Home, the lens landing page; Today is its sub-view and has no shortcut.
    await expect(activeField(page, i === 0 ? "Home" : label).getByRole("heading", { level: 1 })).toHaveText(h1);
  }
  expect(consoleErrors).toEqual([]);
});

test.describe("(d) ⌘K palette", () => {
  test('"rev" + Enter opens Business › Money (the old Revenue view)', async ({ page, theme }) => {
    await openApp(page, { theme });
    await page.keyboard.press("Meta+k");
    const input = page.getByRole("combobox", { name: "Search or ask Amara" });
    await expect(input).toBeFocused();
    await input.fill("rev");
    await page.keyboard.press("Enter");
    await expect(input).toBeHidden();
    await expect(activeField(page, "Business").getByRole("radio", { name: "Money" })).toHaveAttribute("aria-checked", "true");
  });

  test("no match → Ask Amara pre-fills the Chat composer", async ({ page, theme }) => {
    await openApp(page, { theme });
    await page.keyboard.press("Meta+k");
    await page.getByRole("combobox", { name: "Search or ask Amara" }).fill("zz no such command");
    const options = page.getByRole("option");
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText("Ask Amara: “zz no such command”");
    await page.keyboard.press("Enter");
    await expect(page.locator(".landing .hchat #chat-composer")).toHaveValue("zz no such command");
    await expect(page.locator(".landing .hchat #chat-composer")).toBeFocused();
  });

  test("Esc closes", async ({ page, theme }) => {
    await openApp(page, { theme });
    await page.keyboard.press("Meta+k");
    const input = page.getByRole("combobox", { name: "Search or ask Amara" });
    await expect(input).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(input).toBeHidden();
  });
});

/** One left edge (screen-specs › shared container): the content column starts at the same x on
 *  every section. Chat's h1 sits in its middle column, so Chat is measured by its first column;
 *  Settings shares the same column (design-1110 P14.1). */
test("(d) one content edge: h1s, Chat's first column and the sub-view bar line up", async ({ page, theme }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page, { theme });
  const left = async (label: string) => {
    const field = await openView(page, label);
    const target = field.getByRole("heading", { level: 1 }).first();
    await target.waitFor();
    await routeSettled(page); // the box is measured at rest, not part-way through the enter animation
    return (await target.boundingBox())!.x;
  };
  const home = await left("Today");
  for (const label of ["Mind–Body", "Fitness", "Neuro", "Crews", "Business", "Money", "Smart home", "Agents"]) expect(await left(label), label).toBeCloseTo(home, 0);

  // Sub-view bars (Mind–Body, Chat/Activity/Crews, Business) sit on the same edge.
  for (const label of ["Mind–Body", "Agents", "Business"]) {
    await navButton(page, label).click();
    const bar = activeField(page, label).locator(".subview-bar [role=radiogroup]").first();
    if (await bar.count()) expect((await bar.boundingBox())!.x, `${label} bar`).toBeCloseTo(home, 0);
  }
  const history = await openView(page, "Neuro history");
  await expect(history.getByRole("heading", { level: 1 })).toHaveText("Neuro history");
  expect((await history.getByRole("heading", { level: 1 }).boundingBox())!.x).toBeCloseTo(home, 0);

  // Settings joins the shared column at the shared header height (design-1110 P14.1).
  await navButton(page, "Settings").click();
  const settings = (await activeField(page, "Settings").getByRole("heading", { level: 1 }).boundingBox())!.x;
  expect(settings).toBeCloseTo(home, 0);
});

test("(e) Chat: composer ≥ 12px above the window bottom, no horizontal scroll at minimum width", async ({ page, theme }) => {
  for (const size of [{ width: 1440, height: 900 }, { width: 900, height: 620 }]) {
    await page.setViewportSize(size);
    await openApp(page, { theme });
    const composer = (await openHomeChat(page)).locator(".chat-screen__composer");
    await composer.waitFor();
    const box = (await composer.boundingBox())!;
    expect(size.height - (box.y + box.height), `${size.width}×${size.height}`).toBeGreaterThanOrEqual(12);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  }
});
