import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { Footer, footerHasLiveStatus } from "./Footer";
import { SIDEBAR_DEFAULT, Sidebar, clampSidebar } from "./Sidebar";

const online: SystemSnapshot = { ...INITIAL_SNAPSHOT, hermes: { ...INITIAL_SNAPSHOT.hermes, status: "online", model: "hermes-3" } };

describe("Sidebar", () => {
  beforeEach(() => window.localStorage.clear());
  afterEach(() => document.documentElement.style.removeProperty("--shell-sidebar-w"));

  it("defaults to 220px and resizes with the keyboard within 200–280, persisted", () => {
    render(<Sidebar active="terrance" onSelect={() => {}} data={online} />);
    const handle = screen.getByRole("separator", { name: "Resize sidebar" });
    expect(handle).toHaveAttribute("aria-valuenow", String(SIDEBAR_DEFAULT));
    expect(document.documentElement.style.getPropertyValue("--shell-sidebar-w")).toBe("220px");
    fireEvent.keyDown(handle, { key: "End" });
    expect(handle).toHaveAttribute("aria-valuenow", "280");
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(handle).toHaveAttribute("aria-valuenow", "280");
    fireEvent.keyDown(handle, { key: "Home" });
    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    expect(handle).toHaveAttribute("aria-valuenow", "200");
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(window.localStorage.getItem("nexus.sidebarWidth")).toBe("210");
    expect(document.documentElement.style.getPropertyValue("--shell-sidebar-w")).toBe("210px");
  });

  it("restores a stored width (clamped) and marks the active item", () => {
    window.localStorage.setItem("nexus.sidebarWidth", "999");
    render(<Sidebar active="chat" onSelect={() => {}} data={online} />);
    expect(screen.getByRole("separator")).toHaveAttribute("aria-valuenow", "280");
    expect(screen.getByRole("button", { name: /Chat & Agents/ })).toHaveAttribute("aria-current", "page");
  });

  it("shows ⌘-number hints instead of uppercase signal labels", () => {
    const { container } = render(<Sidebar active="terrance" onSelect={() => {}} data={online} />);
    expect(screen.queryByText("HISTORY")).toBeNull();
    expect(screen.queryByText("NOOP")).toBeNull();
    const hints = [...container.querySelectorAll(".nav-shortcut")].map((n) => n.textContent);
    expect(hints.slice(0, 3)).toEqual(["⌘1", "⌘2", "⌘3"]);
    expect(hints.length).toBe(9);
  });

  it("clamps", () => {
    expect([clampSidebar(10), clampSidebar(240.4), clampSidebar(1e6)]).toEqual([200, 240, 280]);
  });
});

describe("Footer", () => {
  it("is hidden when there is no live status", () => {
    const { container } = render(<Footer data={online} busy={false} />);
    expect(container).toBeEmptyDOMElement();
    expect(footerHasLiveStatus(online, false)).toBe(false);
  });

  it("shows refreshing, unavailable sources and Hermes offline", () => {
    const { rerender } = render(<Footer data={online} busy />);
    expect(screen.getByRole("contentinfo")).toHaveTextContent("Refreshing…");
    rerender(<Footer data={{ ...online, unavailableSources: [{ command: "x", label: "X" }] }} busy={false} />);
    expect(screen.getByText("1 unavailable")).toHaveAttribute("title", "X");
    rerender(<Footer data={INITIAL_SNAPSHOT} busy={false} />);
    expect(screen.getByRole("contentinfo")).toHaveTextContent("Hermes offline");
  });
});
