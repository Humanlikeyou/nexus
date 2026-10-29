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
