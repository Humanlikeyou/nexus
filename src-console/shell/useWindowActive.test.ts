import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type FocusHandler = (event: { payload: boolean }) => void;
const tauri = vi.hoisted(() => ({ handler: undefined as FocusHandler | undefined, unlisten: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: (h: FocusHandler) => { tauri.handler = h; return Promise.resolve(tauri.unlisten); },
  }),
}));

import { installWindowActive } from "./useWindowActive";

const root = document.documentElement;
const w = window as unknown as Record<string, unknown>;

describe("installWindowActive (Decision 21)", () => {
  beforeEach(() => { tauri.handler = undefined; tauri.unlisten.mockClear(); delete root.dataset.windowActive; });
  afterEach(() => { delete w.__TAURI_INTERNALS__; vi.restoreAllMocks(); });

  it("initialises from document.hasFocus()", () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    const dispose = installWindowActive();
    expect(root.dataset.windowActive).toBe("false");
    dispose();
  });

  it("flips on a mocked Tauri onFocusChanged and unlistens on cleanup", async () => {
    w.__TAURI_INTERNALS__ = {};
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const dispose = installWindowActive();
    expect(root.dataset.windowActive).toBe("true");
    await Promise.resolve();
    expect(tauri.handler).toBeTypeOf("function");
    tauri.handler!({ payload: false });
    expect(root.dataset.windowActive).toBe("false");
    tauri.handler!({ payload: true });
    expect(root.dataset.windowActive).toBe("true");
    await Promise.resolve();
    dispose();
    expect(tauri.unlisten).toHaveBeenCalledTimes(1);
  });

  it("falls back to DOM focus/blur outside Tauri and removes the listeners on cleanup", () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const dispose = installWindowActive();
    expect(tauri.handler).toBeUndefined();
    window.dispatchEvent(new Event("blur"));
    expect(root.dataset.windowActive).toBe("false");
    window.dispatchEvent(new Event("focus"));
    expect(root.dataset.windowActive).toBe("true");
    dispose();
    window.dispatchEvent(new Event("blur"));
    expect(root.dataset.windowActive).toBe("true");
  });
});

// The CSS side (toolbar mark #91785F/#937D61, "OS" never tertiary, selected icon secondary) is
// asserted on computed styles in test/e2e/wordmark.spec.ts.
