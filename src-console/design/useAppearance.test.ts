import { afterEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ setTheme: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setTheme: tauri.setTheme }) }));

import { syncWindowTheme } from "./useAppearance";

const w = window as unknown as Record<string, unknown>;

describe("syncWindowTheme (native sidebar material follows the app theme)", () => {
  afterEach(() => { delete w.__TAURI_INTERNALS__; tauri.setTheme.mockClear(); });

  it("pins the window appearance to a pinned app theme and releases it for system", () => {
    w.__TAURI_INTERNALS__ = {};
    syncWindowTheme("light");
    syncWindowTheme("dark");
    syncWindowTheme("system");
    expect(tauri.setTheme.mock.calls).toEqual([["light"], ["dark"], [null]]);
  });

  it("does nothing outside Tauri", () => {
    syncWindowTheme("dark");
    expect(tauri.setTheme).not.toHaveBeenCalled();
  });
});
