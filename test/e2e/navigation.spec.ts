import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";

const SECTIONS: { label: string; h1: RegExp }[] = [
  { label: "Home", h1: /^Command Center$/ },
  { label: "Chat & Agents", h1: /^Amara$/ },
  { label: "Fitness", h1: /^Fitness$/ },
  { label: "Neuro", h1: /^Neuro$/ },
  { label: "Business", h1: /^Business$/ },
  { label: "Settings", h1: /^Settings$/ },
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
    ["Chat & Agents", "Activity", (f) => expect(f.getByRole("heading", { level: 1 })).toHaveText("Activity")],
    ["Chat & Agents", "Crews", (f) => expect(f.getByRole("heading", { level: 1 })).toHaveText("Crews")],
    ["Neuro", "History", async (f) => { await expect(f.getByRole("heading", { level: 1 })).toHaveText("History"); await expect(f.getByRole("list", { name: "History" })).toBeVisible(); }],
    ["Business", "Revenue", (f) => expect(f.getByRole("heading", { level: 1 })).toHaveText("Business")],
  ];
  for (const [host, view, check] of SUBVIEWS) {
    test(`(b) ${host} › ${view} via the SegmentedControl`, async ({ page, theme, consoleErrors }) => {
      await openApp(page, { theme });
      await navButton(page, host).click();
      const field = activeField(page, host);
      await field.getByRole("radio", { name: view }).click();
      await expect(field.getByRole("radio", { name: view })).toHaveAttribute("aria-checked", "true");
      await check(field);
      expect(consoleErrors).toEqual([]);
    });
  }

  const ANCHORS: [string, string, string][] = [
    ["Settings", "Settings › Devices", "#devices"],
    ["Settings", "About Amara OS", "#about"],
    ["Home", "Home › Goals", "#goals"],
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

test("(c) ⌘1–6 open the six sections in order", async ({ page, theme, consoleErrors }) => {
  await openApp(page, { theme });
  for (const [i, { label, h1 }] of SECTIONS.entries()) {
    await page.keyboard.press(`Meta+${i + 1}`);
    await expect(activeField(page, label).getByRole("heading", { level: 1 })).toHaveText(h1);
  }
  expect(consoleErrors).toEqual([]);
});

test.describe("(d) ⌘K palette", () => {
  test('"rev" + Enter opens Business › Revenue', async ({ page, theme }) => {
    await openApp(page, { theme });
    await page.keyboard.press("Meta+k");
    const input = page.getByRole("combobox", { name: "Search or ask Amara" });
    await expect(input).toBeFocused();
    await input.fill("rev");
    await page.keyboard.press("Enter");
    await expect(input).toBeHidden();
    await expect(activeField(page, "Business").getByRole("radio", { name: "Revenue" })).toHaveAttribute("aria-checked", "true");
  });

  test("no match → Ask Amara pre-fills the Chat composer", async ({ page, theme }) => {
    await openApp(page, { theme });
    await page.keyboard.press("Meta+k");
    await page.getByRole("combobox", { name: "Search or ask Amara" }).fill("zz no such command");
    const options = page.getByRole("option");
    await expect(options).toHaveCount(1);
    await expect(options.first()).toContainText("Ask Amara: “zz no such command”");
    await page.keyboard.press("Enter");
    await expect(activeField(page, "Chat & Agents").locator("#chat-composer")).toHaveValue("zz no such command");
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
 *  Settings is a centred 640pt preferences column by design and is pinned separately. */
test("(d) one content edge: h1s, Chat's first column and the sub-view bar line up", async ({ page, theme }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page, { theme });
  const left = async (label: string) => {
    await navButton(page, label).click();
    const field = activeField(page, label);
    const target = label === "Chat & Agents" ? field.locator(".chat-screen__agents") : field.getByRole("heading", { level: 1 }).first();
    await target.waitFor();
    return (await target.boundingBox())!.x;
  };
  const home = await left("Home");
  for (const label of ["Fitness", "Neuro", "Business", "Chat & Agents"]) expect(await left(label), label).toBeCloseTo(home, 0);

  // Sub-view bars (Neuro Live/History, Chat/Activity/Crews, Business) sit on the same edge.
  for (const label of ["Neuro", "Chat & Agents", "Business"]) {
    await navButton(page, label).click();
    const bar = activeField(page, label).locator(".subview-bar [role=radiogroup]").first();
    if (await bar.count()) expect((await bar.boundingBox())!.x, `${label} bar`).toBeCloseTo(home, 0);
  }
  await navButton(page, "Neuro").click();
  await activeField(page, "Neuro").getByRole("radio", { name: "History" }).click();
  await expect(activeField(page, "Neuro").getByRole("heading", { level: 1 })).toHaveText("History");
  expect((await activeField(page, "Neuro").getByRole("heading", { level: 1 }).boundingBox())!.x).toBeCloseTo(home, 0);

  // Settings: its own centred column.
  await navButton(page, "Settings").click();
  const settings = (await activeField(page, "Settings").getByRole("heading", { level: 1 }).boundingBox())!.x;
  expect(settings).toBeGreaterThan(home);
});

test("(e) Chat: composer ≥ 12px above the window bottom, no horizontal scroll at minimum width", async ({ page, theme }) => {
  for (const size of [{ width: 1440, height: 900 }, { width: 900, height: 620 }]) {
    await page.setViewportSize(size);
    await openApp(page, { theme });
    await navButton(page, "Chat & Agents").click();
    const composer = activeField(page, "Chat & Agents").locator(".chat-screen__composer");
    await composer.waitFor();
    const box = (await composer.boundingBox())!;
    expect(size.height - (box.y + box.height), `${size.width}×${size.height}`).toBeGreaterThanOrEqual(12);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  }
});
