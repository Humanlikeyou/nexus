// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { render, act } from "@testing-library/react";
import { Profiler } from "react";
import { Clock } from "./Clock";

describe("Clock", () => {
  it("ticks on its own without re-rendering its parent", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 21, 9, 0, 0));
    let parentRenders = 0;
    function Parent() { parentRenders++; return <div><Clock /></div>; }
    const commits: string[] = [];
    const { container } = render(<Profiler id="p" onRender={(_id, phase) => commits.push(phase)}><Parent /></Profiler>);
    const before = container.textContent;
    await act(async () => { await vi.advanceTimersByTimeAsync(61_000); });
    expect(container.textContent).not.toBe(before);
    expect(parentRenders).toBe(1);
  });
});
