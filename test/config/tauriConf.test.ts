import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// Live check 2026-10-29: with the lid closed (window occluded) WebKit suspended the renderer's
// timers and the scheduler (daily summary, prune) stopped. The main window opts out.
const conf = JSON.parse(readFileSync(resolve(__dirname, "../../src-tauri/tauri.conf.json"), "utf8")) as {
  app: { macOSPrivateApi?: boolean; windows: Record<string, unknown>[] };
};

describe("main window config", () => {
  const main = conf.app.windows[0];
  it("never throttles or suspends the renderer in the background (the scheduler runs there)", () => {
    expect(main.backgroundThrottling).toBe("disabled");
  });
  it("keeps the sidebar vibrancy setup", () => {
    expect(main.transparent).toBe(true);
    expect(main.windowEffects).toEqual({ effects: ["sidebar"], state: "followsWindowActiveState" });
    expect(conf.app.macOSPrivateApi).toBe(true);
  });
});

// 2026-11-14 (Taisun: "fix the little icon at the top of my mac"): the menu bar showed the full-colour
// app icon shrunk to a blob, plus a second tray from this config as a solid square. Now: one tray,
// built in lib.rs from a black-on-transparent template of the Amara mark.
describe("menu-bar (tray) icon", () => {
  const lib = readFileSync(resolve(__dirname, "../../src-tauri/src/lib.rs"), "utf8");
  const png = readFileSync(resolve(__dirname, "../../src-tauri/icons/tray-template@2x.png"));
  it("is created once, in lib.rs, not also from tauri.conf.json", () => {
    expect((conf.app as Record<string, unknown>).trayIcon).toBeUndefined();
    expect(lib.match(/TrayIconBuilder::/g)?.length).toBe(1);
  });
  it("uses the template image, marked as a template, never the colour app icon", () => {
    const tray = lib.slice(lib.indexOf("TrayIconBuilder::"), lib.indexOf(".build(app)", lib.indexOf("TrayIconBuilder::")));
    expect(tray).toContain('include_image!("icons/tray-template@2x.png")');
    expect(tray).toContain(".icon_as_template(true)");
    expect(tray).not.toContain("default_window_icon");
  });
  it("the template is a 36x36 (18 pt @2x) RGBA PNG", () => {
    expect(png.subarray(1, 4).toString("latin1")).toBe("PNG");
    expect(png.readUInt32BE(16)).toBe(36);
    expect(png.readUInt32BE(20)).toBe(36);
    expect(png[25]).toBe(6); // colour type 6 = RGBA
  });
});
