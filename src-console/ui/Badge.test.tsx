import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it } from "vitest";
import { Badge, StatusDot } from "./index";

describe("Badge", () => {
  it("renders its text with the tone as a data attribute", () => {
    const ref = createRef<HTMLSpanElement>();
    render(<Badge ref={ref} tone="success" className="x" title="Live data">Live</Badge>);
    const badge = screen.getByText("Live");
    expect(ref.current).toBe(badge);
    expect(badge).toHaveAttribute("data-tone", "success");
    expect(badge).toHaveClass("ui-badge", "x");
    expect(badge).toHaveAttribute("title", "Live data");
  });

  it("defaults to the neutral tone", () => {
    render(<Badge>Beta</Badge>);
    expect(screen.getByText("Beta")).toHaveAttribute("data-tone", "neutral");
  });
});

describe("StatusDot", () => {
  it("always shows a text label; the dot is decorative", () => {
    render(<StatusDot tone="danger" data-testid="s">Offline</StatusDot>);
    const status = screen.getByTestId("s");
    expect(status).toHaveTextContent("Offline");
    expect(status).toHaveAttribute("data-tone", "danger");
    expect(status.querySelector(".ui-status-dot__dot")).toHaveAttribute("aria-hidden", "true");
  });

  it("pulse is a data attribute (animation is CSS-only)", () => {
    render(<StatusDot tone="success" pulse data-testid="s" role="status">Streaming</StatusDot>);
    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("data-pulse", "true");
    expect(status).toHaveTextContent("Streaming");
  });
});

export function _typeChecks() {
  // @ts-expect-error Badge requires a text label
  void <Badge tone="success" />;
  // @ts-expect-error StatusDot requires a text label
  void <StatusDot tone="danger" />;
}
