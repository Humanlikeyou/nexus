import { afterEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ setTheme: vi.fn(() => Promise.resolve()), invoke: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => { while (a.length && a[a.length - 1] === undefined) a.pop(); return (tauri.invoke as (...x: unknown[]) => unknown)(...a); } }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setTheme: tauri.setTheme }) }));

import { syncWindowTheme } from "./useAppearance";

const w = window as unknown as Record<string, unknown>;

describe("syncWindowTheme (native sidebar material follows the app theme)", () => {
  afterEach(() => { delete w.__TAURI_INTERNALS__; tauri.setTheme.mockClear(); tauri.invoke.mockClear(); });

  it("pins the window appearance to a pinned app theme and releases it for system", () => {
    w.__TAURI_INTERNALS__ = {};
    syncWindowTheme("light");
    syncWindowTheme("dark");
    syncWindowTheme("system");
    expect(tauri.setTheme.mock.calls).toEqual([["light"], ["dark"], [null]]);
  });

  it("re-syncs the Dock icon after the window theme is applied", async () => {
    w.__TAURI_INTERNALS__ = {};
    syncWindowTheme("dark");
    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith("sync_dock_icon"));
  });

  it("skips the Dock sync when setTheme fails", async () => {
    w.__TAURI_INTERNALS__ = {};
    tauri.setTheme.mockImplementationOnce(() => Promise.reject(new Error("no window")));
    syncWindowTheme("dark");
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(tauri.invoke).not.toHaveBeenCalled();
  });

  it("does nothing outside Tauri", () => {
    syncWindowTheme("dark");
    expect(tauri.setTheme).not.toHaveBeenCalled();
    expect(tauri.invoke).not.toHaveBeenCalled();
  });
});
