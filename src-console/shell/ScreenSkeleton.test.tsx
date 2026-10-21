// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { ScreenSkeleton } from "./ScreenSkeleton";

describe("ScreenSkeleton", () => {
  it("renders an accessible, spinner-free placeholder", () => {
    const { getByRole, container } = render(<ScreenSkeleton />);
    expect(getByRole("status").getAttribute("aria-label")).toBe("Loading");
    expect(container.querySelectorAll(".screen-skeleton span").length).toBe(4);
  });
});
