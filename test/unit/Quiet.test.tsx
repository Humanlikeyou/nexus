import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Figure, Figures, Line, Lines, More, PageHero, PageSection } from "../../src-console/ui/Quiet";

describe("Quiet", () => {
  it("PageHero is the page's h1 with its statement and optional detail", () => {
    render(<PageHero title="Sleep" statement="You slept well." detail="7h 10m" />);
    expect(screen.getByRole("heading", { level: 1, name: "Sleep" })).toBeInTheDocument();
    expect(screen.getByText("You slept well.")).toBeInTheDocument();
    expect(screen.getByText("7h 10m")).toBeInTheDocument();
  });
  it("PageHero omits the detail line when there is none", () => {
    const { container } = render(<PageHero title="Sleep" statement="Fine." />);
    expect(container.querySelector(".page-hero__detail")).toBeNull();
  });
  it("PageSection turns a folded hero into an h2, so a page never has two h1s", () => {
    const { container } = render(<PageSection id="activity"><PageHero title="Activity" statement="Quiet." /></PageSection>);
    expect(screen.getByRole("heading", { level: 2, name: "Activity" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
    const section = container.querySelector("section#activity");
    expect(section).toHaveAttribute("data-section", "activity");
    expect(container.querySelector(".page-hero--section")).not.toBeNull();
  });
  it("Lines and Line render a labelled list, with detail and trailing only when given", () => {
    const { container } = render(<Lines label="Today"><Line title="Run" detail="5 km" trailing="8:00" /><Line title="Nap" /></Lines>);
    expect(screen.getByRole("list", { name: "Today" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText("5 km")).toBeInTheDocument();
    expect(screen.getByText("8:00")).toBeInTheDocument();
    expect(container.querySelectorAll(".quiet-line__detail")).toHaveLength(1);
    expect(container.querySelectorAll(".quiet-line__trailing")).toHaveLength(1);
  });
  it("Figures pair each label with its value", () => {
    const { container } = render(<Figures className="extra"><Figure label="HRV" value="52 ms" /><Figure label="RHR" value="48" /></Figures>);
    expect(container.querySelector("dl")).toHaveClass("quiet-figures", "extra");
    expect(screen.getByText("HRV").tagName).toBe("DT");
    expect(screen.getByText("52 ms").tagName).toBe("DD");
  });
  it("More is closed by default and open on request", () => {
    const { container, rerender } = render(<More label="Details">body</More>);
    expect(container.querySelector("details")).not.toHaveAttribute("open");
    expect(screen.getByText("Details")).toBeInTheDocument();
    rerender(<More label="Details" open id="d">body</More>);
    expect(container.querySelector("details")).toHaveAttribute("open");
    expect(container.querySelector("details")).toHaveAttribute("id", "d");
  });
});
