import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";
import { Sparkline, sparklineSummary } from "./index";

describe("Sparkline", () => {
  it("is an img labelled with a min/max/last summary", () => {
    const ref = createRef<SVGSVGElement>();
    render(<Sparkline ref={ref} values={[60, 72, 55, 64]} label="HRV" format={(v) => `${v} ms`} className="x" />);
    expect(screen.getByRole("img", { name: "HRV: min 55 ms, max 72 ms, last 64 ms" })).toBeInTheDocument();
    expect(ref.current).toHaveClass("ui-sparkline", "x");
  });

  it("null values are gaps (one subpath per run), not zeros", () => {
    const { container } = render(<Sparkline values={[1, 2, null, 4, 5, null, 7]} label="Recovery" />);
    const d = container.querySelector("path")!.getAttribute("d")!;
    expect(d.match(/M/g)).toHaveLength(3);
    // isolated last point still shows as a dot
    expect(container.querySelectorAll("circle")).toHaveLength(1);
    expect(sparklineSummary([1, null, 7])).toBe("min 1, max 7, last 7");
  });

  it("optional dashed mean line", () => {
    const { rerender } = render(<Sparkline values={[1, 3]} label="x" />);
    expect(screen.queryByTestId("sparkline-mean")).toBeNull();
    rerender(<Sparkline values={[1, 3]} label="x" showMean />);
    expect(screen.getByTestId("sparkline-mean")).toHaveClass("ui-sparkline__mean");
  });

  it("all-null series says no data", () => {
    const { container } = render(<Sparkline values={[null, null]} label="SpO2" />);
    expect(screen.getByRole("img", { name: "SpO2: no data" })).toHaveAttribute("data-empty", "true");
    expect(container.querySelector("path")).toBeNull();
  });
});
