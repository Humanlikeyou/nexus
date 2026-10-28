import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";
import { Meter } from "./index";

describe("Meter", () => {
  it("is a labelled progressbar", () => {
    const ref = createRef<HTMLDivElement>();
    render(<Meter ref={ref} value={0.84 * 100} label="Efficiency" className="x" />);
    const bar = screen.getByRole("progressbar", { name: "Efficiency" });
    expect(bar).toHaveAttribute("aria-valuenow", "84");
    expect(bar).toHaveAttribute("aria-valuemax", "100");
    expect(ref.current).toHaveClass("ui-meter", "x");
    expect(bar.querySelector(".ui-meter__fill")).toHaveStyle({ width: "84%" });
  });

  it("null value is empty, not zero", () => {
    render(<Meter value={null} label="Efficiency" />);
    const bar = screen.getByRole("progressbar");
    expect(bar).not.toHaveAttribute("aria-valuenow");
    expect(bar).toHaveAttribute("aria-valuetext", "No data");
  });

  it("stacked variant summarizes its segments and skips zeros", () => {
    const { container } = render(
      <Meter label="Sleep stages" format={(v) => `${v}m`} segments={[{ label: "Deep", value: 90 }, { label: "REM", value: 100 }, { label: "Light", value: 0 }]} />,
    );
    const img = screen.getByRole("img", { name: "Sleep stages: Deep 90m, REM 100m" });
    expect(img).toHaveAttribute("data-variant", "stacked");
    expect(container.querySelectorAll(".ui-meter__segment")).toHaveLength(2);
  });

  it("stacked variant with no data", () => {
    render(<Meter label="Sleep stages" segments={[]} />);
    expect(screen.getByRole("img", { name: "Sleep stages: no data" })).toHaveAttribute("data-empty", "true");
  });
});
