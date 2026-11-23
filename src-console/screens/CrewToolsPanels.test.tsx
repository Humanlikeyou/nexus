import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CrewToolsPage } from "./CrewToolsPanels";
import { makeSampleCrewTools, unwiredCrewTools } from "../crews/toolGrant";

const page = (adapter = makeSampleCrewTools(() => new Date("2026-10-28T05:00:00Z"), 1000), crew = "smoke") =>
  render(<CrewToolsPage crew={crew} agents={["writer"]} lastRun="last run completed" adapter={adapter} onBack={() => {}} />);

describe("design-1110i Files and terminal switch", () => {
  it("unwired: Needs wiring, no switch, read-only stated, blocked list still shown", () => {
    page(unwiredCrewTools);
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThan(0);
    expect(screen.getByText(/stays read-only/)).toBeInTheDocument();
    expect(screen.getByText("Blocked")).toBeInTheDocument();
    expect(screen.queryByText("Allowed", { selector: ".status-text" })).not.toBeNull(); // kb_query only
  });
  it("off by default; turning on asks first with 1 hour preselected, shows Applying, then On only after confirm", async () => {
    vi.useFakeTimers();
    page();
    const sw = screen.getByRole("switch", { name: "Files and terminal for smoke" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    expect(screen.getByRole("radio", { name: "1 hour" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("⚠ This lets AI run commands on your Mac.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Turn On for 1 Hour" }));
    expect(screen.getAllByText("Applying…").length).toBeGreaterThan(0);
    expect(screen.queryByRole("switch")).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(screen.getByRole("switch", { name: "Files and terminal for smoke" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("Enforced by crew-service")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Audit trail" })).toHaveTextContent("Backend confirmed allowlist");
    expect(screen.getByText("Blocked")).toBeInTheDocument();
  });
  it("no confirmation within 15 s: stays off and says why", async () => {
    vi.useFakeTimers();
    page(makeSampleCrewTools(() => new Date(), 60_000));
    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getByRole("button", { name: "Turn On for 1 Hour" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
    expect(screen.getByRole("alert")).toHaveTextContent("didn't confirm within 15 s");
  });
  it("a crew can only ask: the request waits for Taisun; Deny leaves it off", async () => {
    page(undefined, "sample research");
    expect(screen.getByText("Waiting for you")).toBeInTheDocument();
    expect(screen.queryByRole("switch")).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Deny" })); });
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("list", { name: "Audit trail" })).toHaveTextContent("You denied the request");
  });
});
