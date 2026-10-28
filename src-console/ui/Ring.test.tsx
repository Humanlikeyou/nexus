import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";
import { Ring } from "./index";

describe("Ring", () => {
  it("is a labelled meter with value, min and max", () => {
    const ref = createRef<HTMLDivElement>();
    render(<Ring ref={ref} value={74} label="Recovery" valueText="74%, High" className="x" color="var(--data-recovery-high)"><strong>74</strong></Ring>);
    const meter = screen.getByRole("meter", { name: "Recovery" });
    expect(meter).toHaveAttribute("aria-valuenow", "74");
    expect(meter).toHaveAttribute("aria-valuemin", "0");
    expect(meter).toHaveAttribute("aria-valuemax", "100");
    expect(meter).toHaveAttribute("aria-valuetext", "74%, High");
    expect(ref.current).toHaveClass("ui-ring", "x");
    expect(meter.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(meter.style.getPropertyValue("--ui-ring-color")).toBe("var(--data-recovery-high)");
  });

  it("clamps to the range (Effort 0–100)", () => {
    render(<Ring value={125.4} max={100} label="Effort" />);
    expect(screen.getByRole("meter")).toHaveAttribute("aria-valuenow", "100");
  });

  it("null value renders the track only and says No data", () => {
    render(<Ring value={null} label="Recovery" size="sm" />);
    const meter = screen.getByRole("meter");
    expect(meter).not.toHaveAttribute("aria-valuenow");
    expect(meter).toHaveAttribute("aria-valuetext", "No data");
    expect(meter).toHaveAttribute("data-empty", "true");
    expect(meter).toHaveAttribute("data-size", "sm");
    expect(meter.querySelector(".ui-ring__fill")).toBeNull();
  });
});
