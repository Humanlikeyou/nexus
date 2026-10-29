import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Splash } from "./Splash";

describe("boot splash", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("never shows when the first snapshot lands within 300 ms", () => {
    const { rerender } = render(<Splash loaded={false} />);
    act(() => { vi.advanceTimersByTime(200); });
    rerender(<Splash loaded />);
    act(() => { vi.advanceTimersByTime(2000); });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows after 300 ms, fades 200 ms after load", () => {
    const { rerender } = render(<Splash loaded={false} />);
    act(() => { vi.advanceTimersByTime(350); });
    expect(screen.getByRole("status", { name: "Amara OS is loading" })).toBeInTheDocument();
    const subtitle = screen.getByText("Command Center");
    expect(subtitle).toHaveClass("splash__subtitle");
    expect(subtitle).toHaveAttribute("aria-hidden", "true");
    rerender(<Splash loaded />);
    expect(screen.getByRole("status")).toHaveAttribute("data-fading");
    act(() => { vi.advanceTimersByTime(200); });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("is gone by 1.7 s even if loading stalls", () => {
    render(<Splash loaded={false} />);
    act(() => { vi.advanceTimersByTime(1500); });
    expect(screen.getByRole("status")).toHaveAttribute("data-fading");
    act(() => { vi.advanceTimersByTime(200); });
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("under reduced motion it shows at once and leaves in the same frame (no fade)", () => {
    document.documentElement.dataset.reduceMotion = "true";
    try {
      const { rerender } = render(<Splash loaded={false} />);
      act(() => { vi.advanceTimersByTime(350); });
      expect(screen.getByRole("status")).not.toHaveAttribute("data-fading");
      rerender(<Splash loaded />);
      expect(screen.queryByRole("status")).toBeNull();
    } finally {
      delete document.documentElement.dataset.reduceMotion;
    }
  });
});
