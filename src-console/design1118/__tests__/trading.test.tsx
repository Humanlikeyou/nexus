import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProposalsCard } from "../Trading";
// @ts-expect-error vite ?raw import (no ambient type in this project)
import proposalsSource from "../marketProposals.ts?raw";
// @ts-expect-error vite ?raw import (no ambient type in this project)
import tradingSource from "../Trading.tsx?raw";
import { makeSampleMarket, unwiredMarket, type MarketProposalAdapter } from "../marketProposals";

describe("design-1118a trading proposals: record only, never an order", () => {
  it("Approve opens a second confirm sheet; Cancel records nothing", async () => {
    const a = makeSampleMarket();
    const spy = vi.spyOn(a, "recordChoice");
    render(<ProposalsCard adapter={a} capUsd={10} />);
    fireEvent.click(screen.getAllByRole("button", { name: "Approve" })[0]);
    const sheet = await screen.findByTestId("trade-confirm");
    expect(within(sheet).getByText("Not financial advice.")).toBeTruthy();
    expect(within(sheet).getByText(/No order is sent/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(spy).not.toHaveBeenCalled();
  });

  it("Confirm records the choice and says no order was sent", async () => {
    const a = makeSampleMarket();
    const spy = vi.spyOn(a, "recordChoice");
    render(<ProposalsCard adapter={a} capUsd={10} />);
    fireEvent.click(screen.getAllByRole("button", { name: "Approve" })[0]);
    await screen.findByTestId("trade-confirm");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(spy).toHaveBeenCalledWith("p-aurx", "approved"));
    expect((await screen.findAllByText(/No order was sent/)).length).toBeGreaterThan(0);
  });

  it("the proposal adapter has no order path", () => {
    const methods = Object.keys(makeSampleMarket() as MarketProposalAdapter).sort();
    expect(methods).toEqual(["choices", "market", "newsNote", "proposals", "recordChoice"]);
    const src = String(proposalsSource) + String(tradingSource);
    expect(src).not.toMatch(/invoke\(|fetch\(|placeOrder|submitOrder|sendOrder|executeTrade|broker\w*\(/);
  });

  it("live with no feed shows the designed empty state and invents nothing", () => {
    render(<ProposalsCard adapter={unwiredMarket} />);
    expect(screen.getByText(/No proposals yet/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.getAllByText(/Not financial advice/i).length).toBeGreaterThan(0);
  });
});
