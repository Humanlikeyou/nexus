import { afterEach, describe, expect, it, vi } from "vitest";
import { installVibrancy } from "./vibrancy";

const root = document.documentElement;
const w = window as unknown as Record<string, unknown>;
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";

describe("installVibrancy", () => {
  afterEach(() => { delete w.__TAURI_INTERNALS__; delete root.dataset.vibrancy; vi.restoreAllMocks(); });

  it("marks the root inside Tauri on macOS", () => {
    w.__TAURI_INTERNALS__ = {};
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(MAC);
    installVibrancy();
    expect(root.dataset.vibrancy).toBe("true");
  });

  it("keeps the opaque fallback in a plain browser", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(MAC);
    root.dataset.vibrancy = "true";
    installVibrancy();
    expect(root.dataset.vibrancy).toBeUndefined();
  });

  it("keeps the opaque fallback inside Tauri off macOS", () => {
    w.__TAURI_INTERNALS__ = {};
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    installVibrancy();
    expect(root.dataset.vibrancy).toBeUndefined();
  });
});
