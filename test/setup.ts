import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

// Node 22+ exposes an experimental global localStorage (no backing file) that can
// shadow jsdom's; give jsdom tests a working in-memory Storage when that happens.
function memoryStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k) => (m.has(k) ? m.get(k)! : null),
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => { m.delete(k); },
    setItem: (k, v) => { m.set(k, String(v)); },
  };
}
if (typeof window !== "undefined" && typeof window.localStorage?.getItem !== "function") {
  Object.defineProperty(window, "localStorage", { value: memoryStorage(), configurable: true });
}

afterEach(() => {
  // Vitest runs without globals, so RTL's auto-cleanup is off: unmount explicitly
  // so hooks/listeners from one test can't leak into the next.
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});
