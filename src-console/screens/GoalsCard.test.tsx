import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Goal } from "../types";
import { GoalsCard } from "./GoalsCard";

const goal = (id: string, o: Partial<Goal> = {}): Goal => ({ id, title: `Goal ${id}`, category: "health", target: 100, current: 25, unit: "km", deadline: null, ...o });

describe("Home › Goals card (§6)", () => {
  it("summarizes progress with Meters and handles target 0", () => {
    render(<GoalsCard goals={[goal("a", { current: 80, deadline: "2026-12-31" }), goal("b"), goal("c", { target: 0 })]} />);
    const card = screen.getByRole("region", { name: "Goals" });
    expect(within(card).getByText("3 active")).toBeInTheDocument();
    expect(within(card).getByText("Goal a · 80%")).toBeInTheDocument();
    expect(within(card).getAllByRole("progressbar")).toHaveLength(2);
    expect(within(card).getByText("No target")).toBeInTheDocument();
    expect(within(card).getByText("80 / 100 km")).toBeInTheDocument();
    expect(within(card).getByText(/Health · Target Dec 31, 2026/)).toBeInTheDocument();
    expect(within(card).queryByText(/Strain|readiness/i)).toBeNull();
  });

  it("empty and error states", () => {
    const { unmount } = render(<GoalsCard goals={[]} />);
    expect(screen.getByText("No goals yet")).toBeInTheDocument();
    unmount();
    const onRetry = vi.fn();
    render(<GoalsCard goals={[]} failed onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
