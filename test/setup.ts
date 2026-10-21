import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

afterEach(() => {
  // Vitest runs without globals, so RTL's auto-cleanup is off: unmount explicitly
  // so hooks/listeners from one test can't leak into the next.
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});
