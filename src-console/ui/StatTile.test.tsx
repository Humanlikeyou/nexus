import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";
import { StatTile } from "./index";

describe("StatTile", () => {
  it("is a term/definition pair with the value and unit", () => {
    const ref = createRef<HTMLDListElement>();
    render(<StatTile ref={ref} label="HRV" value={62} unit="ms" className="x" />);
    expect(screen.getByRole("term")).toHaveTextContent("HRV");
    const value = screen.getByRole("definition");
    expect(value).toHaveTextContent("62ms");
    expect(value).toHaveClass("ui-stat__value");
    expect(ref.current?.tagName).toBe("DL");
    expect(ref.current).toHaveClass("ui-stat", "x");
  });

  it("trend has a text alternative; the arrow and short value are hidden from AT", () => {
    render(<StatTile label="Recovery" value={74} unit="%" trend={{ direction: "up", value: "+6%", label: "Up 6% from yesterday", sentiment: "positive" }} />);
    const [, trend] = screen.getAllByRole("definition");
    expect(trend).toHaveAttribute("data-direction", "up");
    expect(trend).toHaveAttribute("data-sentiment", "positive");
    expect(screen.getByText("Up 6% from yesterday")).toHaveClass("ui-visually-hidden");
    expect(screen.getByText("+6%")).toHaveAttribute("aria-hidden", "true");
    expect(trend.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    // Accessible text of the trend is the alternative, not the glyph.
    expect(trend.textContent).toContain("Up 6% from yesterday");
  });

  it("flat trend defaults to neutral sentiment", () => {
    render(<StatTile label="Strain" value="10.2" trend={{ direction: "flat", label: "No change" }} />);
    const [, trend] = screen.getAllByRole("definition");
    expect(trend).toHaveAttribute("data-direction", "flat");
    expect(trend).toHaveAttribute("data-sentiment", "neutral");
  });
});

export function _typeChecks() {
  // @ts-expect-error a trend needs a text alternative
  void <StatTile label="x" value={1} trend={{ direction: "up" }} />;
}
