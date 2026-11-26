import { render, waitFor, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { sampleMoney } from "./adapters";
import { FULL, VIEW } from "./fixtures";
import { MoneyAccounts, MoneyFollowUps, MoneyOverviewSection, MoneyTake } from "./MoneyView";
import { MoneyEntriesPanel } from "./MoneyEntriesPanel";
import type { MoneyEntries } from "./entries";

const YEN = /[¥￥]/;
describe("no yen sign in rendered money views", () => {
  it("rendered money views contain no yen glyph", async () => {
    const ma = sampleMoney();
    const api: MoneyEntries = { add: async () => { throw new Error("no"); }, undo: async () => undefined, list: async () => ({ entries: [], skipped_lines: 0 }), monthView: async () => VIEW, setSavingsGoal: async () => undefined, setRate: async () => undefined };
    const { container } = render(<><MoneyAccounts adapter={ma} /><MoneyOverviewSection adapter={ma} currency="JPY" onCurrency={() => undefined} /><MoneyFollowUps adapter={ma} /><MoneyTake adapter={ma} /><MoneyEntriesPanel api={api} /></>);
    await waitFor(() => expect(screen.getByRole("heading", { name: /Income and spending by month/ })).toBeInTheDocument());
    expect(screen.getByText("Amara's money read")).toBeInTheDocument();
    expect(container.textContent).not.toMatch(YEN);
    expect(container.textContent).toContain("JPY");
    expect(FULL.currency).toBe("JPY");
  });
});
