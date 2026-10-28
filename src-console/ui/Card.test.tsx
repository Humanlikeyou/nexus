import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";
import { Button, Card } from "./index";

describe("Card", () => {
  it("with a title is a section labelled by its heading (a named region)", () => {
    render(<Card title="Recovery" accessory={<Button size="small">Details</Button>}>Body</Card>);
    const region = screen.getByRole("region", { name: "Recovery" });
    expect(region.tagName).toBe("SECTION");
    const heading = screen.getByRole("heading", { level: 2, name: "Recovery" });
    expect(region).toHaveAttribute("aria-labelledby", heading.id);
    expect(screen.getByRole("button", { name: "Details" })).toBeInTheDocument();
  });

  it("without a title has no aria-labelledby", () => {
    render(<Card data-testid="c">Body</Card>);
    const card = screen.getByTestId("c");
    expect(card.tagName).toBe("SECTION");
    expect(card).not.toHaveAttribute("aria-labelledby");
    expect(screen.queryByRole("heading")).toBeNull();
  });

  it("supports as, headingLevel, padding, elevation, className, ref and native attributes", () => {
    const ref = createRef<HTMLElement>();
    render(<Card ref={ref} as="article" title="Sleep" headingLevel={3} padding="none" elevation={2} className="x" id="sleep-card">Body</Card>);
    const card = screen.getByRole("article", { name: "Sleep" });
    expect(ref.current).toBe(card);
    expect(card).toHaveAttribute("id", "sleep-card");
    expect(card).toHaveAttribute("data-padding", "none");
    expect(card).toHaveAttribute("data-elevation", "2");
    expect(card).toHaveClass("ui-card", "x");
    expect(screen.getByRole("heading", { level: 3, name: "Sleep" })).toBeInTheDocument();
  });

  it("defaults to padding md and elevation 1; a caller's aria-labelledby wins", () => {
    render(<><h2 id="ext">External</h2><Card title="Inner" aria-labelledby="ext">Body</Card></>);
    const region = screen.getByRole("region", { name: "External" });
    expect(region).toHaveAttribute("data-padding", "md");
    expect(region).toHaveAttribute("data-elevation", "1");
  });
});
