import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { SECRET_PATTERN, resetSampleSites } from "./adapters";
import { SignedInSites } from "./SignedInSites";

describe("Settings › Services signed-in sites", () => {
  beforeEach(() => {
    localStorage.setItem("nexus.sampleAdapters", "1");
    resetSampleSites();
  });
  afterEach(() => {
    cleanup();
    localStorage.removeItem("nexus.sampleAdapters");
    resetSampleSites();
  });

  it("lists status only, with Sign out, and never shows secrets", async () => {
    const { container } = render(<SignedInSites />);
    expect(screen.getByRole("heading", { name: "Sites you're signed in to inside Amara" })).toBeInTheDocument();
    expect(screen.getByText("instagram.com")).toBeInTheDocument();
    expect(document.querySelector('[data-site="instagram"]')?.textContent).toMatch(/Signed in/);
    expect(container.innerHTML).not.toMatch(SECRET_PATTERN);
    await userEvent.click(screen.getByRole("button", { name: "Sign out of Instagram" }));
    expect(await screen.findByText(/Signed out of instagram.com/)).toBeInTheDocument();
    expect(container.innerHTML).not.toMatch(SECRET_PATTERN);
  });

  it("live mode is a Needs wiring state with no invented sites or secrets", () => {
    localStorage.removeItem("nexus.sampleAdapters");
    const { container } = render(<SignedInSites />);
    expect(screen.getAllByText(/Needs wiring/).length).toBeGreaterThan(0);
    expect(screen.queryByText("instagram.com")).not.toBeInTheDocument();
    expect(container.innerHTML).not.toMatch(SECRET_PATTERN);
  });
});
