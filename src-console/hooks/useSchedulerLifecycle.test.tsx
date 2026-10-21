// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { StrictMode, createElement, type ReactNode } from "react";
import { renderHook } from "@testing-library/react";

const s = vi.hoisted(() => ({ startScheduler: vi.fn(), stopScheduler: vi.fn() }));
vi.mock("@/terrance/system/scheduler", () => s);

import { useSchedulerLifecycle } from "./useSchedulerLifecycle";

const active = () => s.startScheduler.mock.calls.length - s.stopScheduler.mock.calls.length;

beforeEach(() => {
  s.startScheduler.mockClear();
  s.stopScheduler.mockClear();
  delete window.__nexusSchedulerStarted;
});

describe("useSchedulerLifecycle", () => {
  it("two concurrent mounts start the scheduler only once", () => {
    const a = renderHook(() => useSchedulerLifecycle());
    const b = renderHook(() => useSchedulerLifecycle());
    expect(s.startScheduler).toHaveBeenCalledTimes(1);
    b.unmount(); // non-owner unmount must not stop it
    expect(s.stopScheduler).not.toHaveBeenCalled();
    a.unmount();
    expect(s.stopScheduler).toHaveBeenCalledTimes(1);
    expect(window.__nexusSchedulerStarted).toBeUndefined();
  });

  it("StrictMode double-mount leaves exactly one running instance", () => {
    const wrapper = ({ children }: { children: ReactNode }) => createElement(StrictMode, null, children);
    const { unmount } = renderHook(() => useSchedulerLifecycle(), { wrapper });
    expect(active()).toBe(1);
    expect(window.__nexusSchedulerStarted).toBeDefined();
    unmount();
    expect(active()).toBe(0);
  });

  it("does not start when a guard is already set (e.g. HMR re-mount)", () => {
    window.__nexusSchedulerStarted = Symbol("other");
    renderHook(() => useSchedulerLifecycle());
    expect(s.startScheduler).not.toHaveBeenCalled();
  });
});
