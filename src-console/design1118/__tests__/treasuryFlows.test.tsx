import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TreasuryBody } from "../TreasuryView";
import { TreasuryFinancesLink, VentureTreasuryCard } from "../TreasuryLinks";
import { toMapping, type TreasuryTools } from "../TreasuryTools";
import { liveTreasury, liveTreasuryMonitor, unwiredTreasury, unwiredTreasuryMonitor } from "../../business/treasury";
import { UNWIRED } from "../../lib/sampleAdapters";

const stream = { id: "s1", ventureId: "v1", name: "Shop", streamType: "product_sales", status: "active", setupDate: null, notes: null, createdAt: "", ventureMissing: false };
const snap = (flags: unknown[] = []) => ({
  t: liveTreasury({ streams: [stream], months: [], trend: [], perHour: [], fx: null }),
  m: liveTreasuryMonitor({ flags, monitor: { status: { state: "ok", asOf: "2026-11-19" } }, review: null }),
});
const mk = (over: Record<string, unknown> = {}) => {
  const calls = {
    treasuryStreamAdd: vi.fn(async () => stream),
    treasuryImportPreview: vi.fn(async () => ({ rowsSeen: 3, wouldAdd: 2, wouldSkip: 0, failed: 1, errors: [{ row: 3, code: "bad_date" }] })),
    treasuryImportPick: vi.fn(async () => ({ added: 2, skipped: 0, failed: 1, importId: "i1" })),
    treasuryAttentionSet: vi.fn(async () => null),
    treasuryFlagsAck: vi.fn(async () => null),
    treasuryMonitorRunNow: vi.fn(async () => ({ ranAt: "t", flags: 1, newFlags: 1, cleared: 0, status: { state: "flags", asOf: null }, relay: "none" })),
    treasuryReviewBuild: vi.fn(async () => ({ month: "2026-10", state: "ok", text: "x", reason: null, builtAt: "t", rows: [] })),
    ...over,
  };
  const tools = { calls, ventures: [{ id: "v1", name: "Taiyo" }], reviewMonth: "2026-10", onChanged: vi.fn() } as unknown as TreasuryTools;
  return { calls, tools };
};
const IDS: Record<string, string> = { "Venture": "tr-venture", "Name": "tr-sname", "Date column": "tr-m-date", "Amount column": "tr-m-amount",
  "Currency when there is no column": "tr-m-cur", "Day": "tr-t-day", "Minutes": "tr-t-mins" };
const type = (label: string, v: string, id?: string) => {
  const el = document.getElementById(id ?? (label === "Stream" ? "tr-imp-stream" : IDS[label])) as HTMLElement;
  fireEvent.change(el, { target: { value: v } });
};

describe("Treasury flows", () => {
  it("add stream: calls the command, shows the result, reloads; an error shows a plain line", async () => {
    const { calls, tools } = mk();
    const s = snap();
    render(<TreasuryBody treasury={s.t} monitor={s.m} tools={tools} />);
    type("Venture", "v1"); type("Name", "Shop"); fireEvent.click(screen.getByRole("button", { name: "Add stream" }));
    expect(await screen.findByText("Added the stream Shop.")).toBeTruthy();
    expect(calls.treasuryStreamAdd).toHaveBeenCalledWith("v1", "Shop", "product_sales", null, null);
    expect(tools.onChanged).toHaveBeenCalled();
    calls.treasuryStreamAdd.mockRejectedValueOnce("unknown_venture");
    fireEvent.click(screen.getByRole("button", { name: "Add stream" }));
    // name was cleared after success, so re-enter
    type("Name", "Again"); fireEvent.click(screen.getByRole("button", { name: "Add stream" }));
    expect(await screen.findByText(/not in the business ledger/)).toBeTruthy();
  });
  it("add stream without a ledger says Needs wiring", () => {
    const { tools } = mk(); tools.ventures = null;
    render(<TreasuryBody treasury={unwiredTreasury} monitor={unwiredTreasuryMonitor} tools={tools} />);
    expect(screen.getByText(/Needs wiring/)).toBeTruthy();
  });
  it("import: preview first, nothing written until Import; cancel writes nothing", async () => {
    const { calls, tools } = mk();
    const s = snap();
    render(<TreasuryBody treasury={s.t} monitor={s.m} tools={tools} />);
    type("Stream", "s1"); type("Date column", "Date"); type("Amount column", "Total"); type("Currency when there is no column", "JPY");
    fireEvent.click(screen.getByRole("button", { name: "Choose file and preview" }));
    expect(await screen.findByTestId("tr-preview")).toBeTruthy();
    expect(screen.getByTestId("tr-preview").textContent).toContain("2 would be added");
    expect(screen.getByText("date not understood")).toBeTruthy();
    expect(calls.treasuryImportPreview).toHaveBeenCalledWith("s1", "generic", { dateColumn: "Date", amountColumn: "Total", currencyColumn: null, defaultCurrency: "JPY", refColumn: null, dateFormat: "ymd" });
    expect(calls.treasuryImportPick).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByTestId("tr-preview")).toBeNull();
    expect(calls.treasuryImportPick).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Choose file and preview" }));
    fireEvent.click(await screen.findByRole("button", { name: "Import 2 rows" }));
    expect(await screen.findByText(/Imported: 2 added/)).toBeTruthy();
    expect(calls.treasuryImportPick).toHaveBeenCalledTimes(1);
    expect(tools.onChanged).toHaveBeenCalled();
  });
  it("import: editing the mapping drops the preview; cancelled pick and bad header show plain lines", async () => {
    const { calls, tools } = mk();
    const s = snap();
    render(<TreasuryBody treasury={s.t} monitor={s.m} tools={tools} />);
    type("Stream", "s1"); type("Date column", "Date"); type("Amount column", "Total"); type("Currency when there is no column", "USD");
    fireEvent.click(screen.getByRole("button", { name: "Choose file and preview" }));
    await screen.findByTestId("tr-preview");
    type("Amount column", "Amt");
    expect(screen.queryByTestId("tr-preview")).toBeNull();
    calls.treasuryImportPreview.mockRejectedValueOnce("cancelled");
    fireEvent.click(screen.getByRole("button", { name: "Choose file and preview" }));
    expect(await screen.findByText(/No file was chosen/)).toBeTruthy();
    calls.treasuryImportPreview.mockRejectedValueOnce("bad_header");
    fireEvent.click(screen.getByRole("button", { name: "Choose file and preview" }));
    expect(await screen.findByText(/not in the file's header row/)).toBeTruthy();
    expect(calls.treasuryImportPick).not.toHaveBeenCalled();
  });
  it("mapping needs a date column, an amount column and a currency", () => {
    const f = { date: "d", amount: "a", currencyCol: "", defaultCur: "", ref: "", fmt: "ymd" };
    expect(toMapping(f)).toBeNull();
    expect(toMapping({ ...f, currencyCol: "c" })?.currencyColumn).toBe("c");
  });
  it("log time: validates minutes, calls the command, confirms", async () => {
    const { calls, tools } = mk();
    const s = snap();
    render(<TreasuryBody treasury={s.t} monitor={s.m} tools={tools} />);
    type("Stream", "s1", "tr-t-stream"); type("Day", "2026-11-18"); type("Minutes", "2000");
    expect((screen.getByRole("button", { name: "Log time" }) as HTMLButtonElement).disabled).toBe(true);
    type("Minutes", "45"); fireEvent.click(screen.getByRole("button", { name: "Log time" }));
    expect(await screen.findByText(/Logged 45 minutes for 2026-11-18/)).toBeTruthy();
    expect(calls.treasuryAttentionSet).toHaveBeenCalledWith("s1", "2026-11-18", 45);
  });
  it("acknowledge a flag: calls ack, shows the line, reloads; no Undo is offered", async () => {
    const { calls, tools } = mk();
    const s = snap([{ id: "stale_data:s1", kind: "stale_data", streamId: "s1", sequenceId: null, since: "2026-11-10T00:00:00", detail: "d", acked: false }]);
    render(<TreasuryBody treasury={s.t} monitor={s.m} tools={tools} />);
    fireEvent.click(screen.getByRole("button", { name: "Mark Data is stale as seen" }));
    await waitFor(() => expect(calls.treasuryFlagsAck).toHaveBeenCalledWith("stale_data:s1"));
    expect(await screen.findByText(/Marked as seen/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
    expect(tools.onChanged).toHaveBeenCalled();
  });
  it("check now and build review end in a visible result; a hidden review says why", async () => {
    const { calls, tools } = mk();
    const s = snap();
    render(<TreasuryBody treasury={s.t} monitor={s.m} tools={tools} />);
    fireEvent.click(screen.getByRole("button", { name: "Check now" }));
    expect(await screen.findByText("Checked just now: 1 to look at, 1 new, 0 cleared.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Build review for 2026-10" }));
    expect(await screen.findByText(/review for 2026-10 is ready/)).toBeTruthy();
    expect(calls.treasuryReviewBuild).toHaveBeenCalledWith("2026-10");
    calls.treasuryReviewBuild.mockResolvedValueOnce({ month: "2026-10", state: "hidden", text: null, reason: "mismatch", builtAt: null, rows: [] } as never);
    fireEvent.click(screen.getByRole("button", { name: "Build review for 2026-10" }));
    expect(await screen.findByText(/held back because its numbers did not match/)).toBeTruthy();
    calls.treasuryMonitorRunNow.mockRejectedValueOnce("busy");
    fireEvent.click(screen.getByRole("button", { name: "Check now" }));
    expect(await screen.findByText(/Another Treasury job is running/)).toBeTruthy();
  });
  it("without tools the page stays read-only", () => {
    const s = snap();
    render(<TreasuryBody treasury={s.t} monitor={s.m} />);
    expect(screen.queryByTestId("tr-tools")).toBeNull();
  });
});

describe("Treasury reaches Finances and ventures", () => {
  const ent = (f: unknown) => ({ finances: () => (f ? { state: "live", value: f } : UNWIRED) }) as never;
  it("finances: monitor chip with as-of and an Open Treasury link", () => {
    const onOpen = vi.fn();
    render(<TreasuryFinancesLink onOpen={onOpen} adapter={ent({ monitorState: "ok", monitorAsOf: "2026-11-19" })} />);
    expect(screen.getByTestId("finances-monitor").textContent).toBe("All good, ok as of 2026-11-19.");
    fireEvent.click(screen.getByRole("button", { name: "Open Treasury ›" }));
    expect(onOpen).toHaveBeenCalledWith({ id: "enterprise", view: "treasury" });
  });
  it("finances: stale is never shown as ok, and nothing renders without a ledger", () => {
    const { container, rerender } = render(<TreasuryFinancesLink adapter={ent({ monitorState: "stale", monitorAsOf: "2026-11-01" })} />);
    expect(screen.getByTestId("finances-monitor").textContent).toBe("Some data is stale.");
    rerender(<TreasuryFinancesLink adapter={ent(null)} />);
    expect(container.textContent).toBe("");
  });
  it("venture: shows backend figures with currency codes, never a yen sign, and nothing when empty", () => {
    const v = { incomeMonth: [{ streamId: "s1", currency: "JPY", month: "2026-11", totalMinor: 12000 }, { streamId: "s2", currency: "USD", month: "2026-11", totalMinor: null }],
      attentionMinutes30d: 120, perHour30d: [{ streamId: "s1", minutes: 120, incomeMinor: 12000, currency: "JPY", fxAsOf: null, fxStale: false, perHourMinor: 6000, reason: null }] } as never;
    const { container, rerender } = render(<VentureTreasuryCard venture={v} />);
    expect(container.textContent).toContain("JPY 12,000");
    expect(container.textContent).toContain("2 hours");
    expect(container.textContent).toContain("JPY 6,000 an hour");
    expect(container.textContent).not.toContain("¥");
    rerender(<VentureTreasuryCard venture={{ incomeMonth: [], attentionMinutes30d: null, perHour30d: [] } as never} />);
    expect(container.textContent).toBe("");
  });
});
