import { activeField, expect, navButton, openApp, test } from "./fixtures/tauri-mock";

const SECTIONS: { label: string; h1: RegExp }[] = [
  { label: "Home", h1: /^Good (morning|afternoon|evening)/ },
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
    // Neuro › History (SessionsScreen) has no h1; its session list is the landmark.
    ["Neuro", "History", (f) => expect(f.getByRole("list", { name: "History" })).toBeVisible()],
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
