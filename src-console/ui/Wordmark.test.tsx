import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Wordmark, WORDMARK_PATH } from "./Wordmark";

describe("Wordmark", () => {
  it("is an img named Amara OS, drawn with stroked paths in currentColor", () => {
    const { container } = render(<Wordmark size="about" />);
    const svg = screen.getByRole("img", { name: "Amara OS" });
    expect(svg.tagName.toLowerCase()).toBe("svg");
    expect(svg).toHaveAttribute("fill", "none");
    expect(svg).toHaveAttribute("stroke", "currentColor");
    expect(svg).toHaveAttribute("stroke-linecap", "round");
    expect(container.querySelector("path")).toHaveAttribute("d", WORDMARK_PATH);
    expect(container.querySelector("text")).toBeNull();
  });

  it.each([
    ["sidebar", 95.28, 13.68, "12.5"],
    ["about", 254.08, 36.48, "11"],
    ["splash", 381.12, 54.72, "11"],
  ] as const)("%s: cap-height driven size and stroke", (size, w, h, stroke) => {
    const { container } = render(<Wordmark size={size} />);
    const svg = container.querySelector("svg")!;
    expect(Number(svg.getAttribute("width"))).toBeCloseTo(w, 1);
    expect(Number(svg.getAttribute("height"))).toBeCloseTo(h, 1);
    expect(svg).toHaveAttribute("stroke-width", stroke);
    expect(svg).toHaveClass("wordmark", `wordmark--${size}`);
  });

  it("decorative hides it from assistive tech", () => {
    const { container } = render(<Wordmark size="splash" decorative />);
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(screen.queryByRole("img")).toBeNull();
  });
});
