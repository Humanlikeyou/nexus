import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { liveBusiness } from "../business/liveLedger";
import { amountText, makeLedgerWrites, parseMinor, type LedgerChange, type LedgerWrites } from "../business/ledgerWrite";
import { BusinessField } from "./BusinessScreen";
import { RevenueField } from "./RevenueScreen";
import { MoneyForm, RateForm, VentureForm, VenturePick, withVenture } from "./EnterpriseForms";
import { LedgerHistory, changeText } from "./EnterpriseStart";
import type { AddAdapter } from "../adds/flow";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

const change = { id: "c1", at: "2026-11-19T10:00:00+09:00", action: "venture_add", subject: "shop", label: "Shop", undone: false };
const fake = (over: Partial<LedgerWrites> = {}): LedgerWrites => ({
  saveVenture: vi.fn(async () => change), archiveVenture: vi.fn(async () => change), setRate: vi.fn(async () => change),
  undoChange: vi.fn(async () => true), history: vi.fn(async () => []),
  addEntry: vi.fn(async () => ({ id: "e1", ventureId: "shop", date: "2026-11-19", amountMinor: 1, currency: "JPY" })),
  undoEntry: vi.fn(async () => true), ventureMonth: vi.fn(async () => []), ...over,
});
const ledger = (extra: object = {}) => ({
  version: 1 as const, fx: null, funds: [], fundingRequests: [], taxFlags: [], voteRules: null, revenueTargetJpy: null,
  ventures: [{ id: "shop", name: "Shop", stage: "Live", health: "on_track" as const, progress: null, nextStep: "Order jars", note: "Sells jars", targetJpy: 300000, detail: null, ...extra }],
});
const snap = (l: unknown): SystemSnapshot => ({ ...INITIAL_SNAPSHOT, refreshedAt: INITIAL_SNAPSHOT.refreshedAt + 1, ledger: l as SystemSnapshot["ledger"] });

describe("ledgerWrite helpers", () => {
  it("parses amounts into minor units and rejects junk", () => {
    expect(parseMinor("12,000", "JPY")).toBe(12000);
    expect(parseMinor("12.5", "JPY")).toBeNull();
    expect(parseMinor("12.50", "USD")).toBe(1250);
    expect(parseMinor("-3", "USD")).toBeNull();
    expect(parseMinor("abc", "JPY")).toBeNull();
  });
  it("shows yen as JPY text, never a yen sign", () => {
    expect(amountText(612000, "JPY")).toBe("612,000 JPY");
    expect(amountText(1250, "USD")).toBe("12.50 USD");
  });
  it("calls the right commands and tells the app to reload", async () => {
    const call = vi.fn(async (..._a: unknown[]) => change);
    const notify = vi.fn();
    const w = makeLedgerWrites(call as never, notify);
    await w.saveVenture({ id: null, name: "A", stage: "Idea", oneLiner: "", nextStep: null, monthlyTargetMinor: null, currency: "JPY" });
    await w.setRate(150, "2026-11-19");
    await w.addEntry({ ventureId: "a", date: "2026-11-19", amountMinor: 5, currency: "JPY", kind: "cost", note: null });
    expect(call.mock.calls.map((c) => c[0])).toEqual(["ledger_venture_save", "ledger_fx_set", "treasury_entry_add"]);
    expect(notify).toHaveBeenCalledTimes(3);
    await w.ventureMonth("2026-11");
    expect(notify).toHaveBeenCalledTimes(3);
  });
});

describe("VentureForm", () => {
  it("asks for a name, then saves, confirms with Undo", async () => {
    const w = fake();
    render(<VentureForm writes={w} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Add venture" }));
    expect(screen.getByText("Give the venture a name.")).toBeInTheDocument();
    expect(w.saveVenture).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Shop" } });
    fireEvent.change(screen.getByLabelText("What it is"), { target: { value: "Sells jars" } });
    fireEvent.change(screen.getByLabelText(/Monthly revenue target/), { target: { value: "300,000" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add venture" })); });
    expect(w.saveVenture).toHaveBeenCalledWith({ id: null, name: "Shop", stage: "Idea", oneLiner: "Sells jars", nextStep: null, monthlyTargetMinor: 300000, currency: "JPY" });
    expect(screen.getByRole("status")).toHaveTextContent("Added Shop.");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Undo" })); });
    expect(w.undoChange).toHaveBeenCalledWith("c1");
    expect(screen.getByRole("status")).toHaveTextContent("Undone.");
  });
  it("shows the backend's plain error and says saving needs the app when there is no backend", async () => {
    const w = fake({ saveVenture: vi.fn(async () => { throw "ledger: keep the description to 140 characters"; }) });
    const { unmount } = render(<VentureForm writes={w} onClose={() => {}} />);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "X" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add venture" })); });
    expect(screen.getByRole("alert")).toHaveTextContent("Keep the description to 140 characters.");
    unmount();
    render(<VentureForm writes={null} onClose={() => {}} />);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "X" } });
    fireEvent.click(screen.getByRole("button", { name: "Add venture" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Amara OS app");
  });
});

describe("MoneyForm", () => {
  it("logs a cost in minor units and confirms in plain words", async () => {
    const w = fake();
    render(<MoneyForm writes={w} ventures={[{ id: "shop", name: "Shop", currency: "JPY" }]} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("radio", { name: "A cost" }));
    fireEvent.change(screen.getByLabelText("Amount"), { target: { value: "2,500" } });
    fireEvent.change(screen.getByLabelText("Date"), { target: { value: "2026-11-19" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Log cost" })); });
    expect(w.addEntry).toHaveBeenCalledWith({ ventureId: "shop", date: "2026-11-19", amountMinor: 2500, currency: "JPY", kind: "cost", note: null });
    expect(screen.getByRole("status")).toHaveTextContent("Logged a cost of 2,500 JPY for Shop on 2026-11-19.");
    expect(document.body.textContent).not.toMatch(/¥/);
  });
  it("rejects a bad amount without calling the backend, and with no ventures points to Add a venture", () => {
    const w = fake();
    const { unmount } = render(<MoneyForm writes={w} ventures={[{ id: "shop", name: "Shop" }]} onClose={() => {}} />);
    fireEvent.change(screen.getByLabelText("Amount"), { target: { value: "12.5" } });
    fireEvent.click(screen.getByRole("button", { name: "Log income" }));
    expect(screen.getByText(/whole number of yen/)).toBeInTheDocument();
    expect(w.addEntry).not.toHaveBeenCalled();
    unmount();
    const onAdd = vi.fn();
    render(<MoneyForm writes={w} ventures={[]} onClose={() => {}} onAddVenture={onAdd} />);
    fireEvent.click(screen.getByRole("button", { name: "Add a venture" }));
    expect(onAdd).toHaveBeenCalled();
  });
});

describe("RateForm", () => {
  it("saves a hand-entered rate with its date and refuses nonsense", async () => {
    const w = fake();
    render(<RateForm writes={w} />);
    fireEvent.change(screen.getByLabelText("Yen per 1 US dollar"), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save rate" }));
    expect(w.setRate).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Yen per 1 US dollar"), { target: { value: "150.25" } });
    fireEvent.change(screen.getByLabelText("Rate as of"), { target: { value: "2026-11-19" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Save rate" })); });
    expect(w.setRate).toHaveBeenCalledWith(150.25, "2026-11-19");
    expect(screen.getByRole("status")).toHaveTextContent("1 USD = 150.25 JPY as of 2026-11-19");
  });
});

describe("Enterprise start page", () => {
  it("lists a venture with its month, next step and a Log money action; Add a venture opens the form", async () => {
    const w = fake({ ventureMonth: vi.fn(async () => [{ ventureId: "shop", currency: "JPY", month: "2026-11", incomeMinor: 12000, costMinor: 2500, rows: 2 }]) });
    const data = snap(ledger());
    render(<BusinessField data={data} adapter={liveBusiness(data.ledger)} writes={w} />);
    const row = screen.getByTestId("venture-row-shop");
    await waitFor(() => expect(row).toHaveTextContent("in 12,000 JPY · out 2,500 JPY"));
    expect(row).toHaveTextContent("Next: Order jars");
    expect(row).toHaveTextContent("Sells jars");
    fireEvent.click(within(row).getByRole("button", { name: "Log money" }));
    expect(screen.getByRole("form", { name: "Log income or a cost" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add a venture" }));
    expect(screen.getByRole("form", { name: "Add a venture" })).toBeInTheDocument();
  });
  it("hides archived ventures from the list", () => {
    const data = snap(ledger({ archived: true }));
    render(<BusinessField data={data} adapter={liveBusiness(data.ledger)} writes={fake()} />);
    expect(screen.queryByTestId("venture-row-shop")).toBeNull();
    expect(screen.getByText(/No ventures yet/)).toBeInTheDocument();
  });
  it("Ventures: no Not connected placeholder; archived ventures can be restored", async () => {
    const w = fake();
    const data = snap(ledger({ archived: true }));
    render(<BusinessField view="ventures" data={data} adapter={liveBusiness(data.ledger)} writes={w} />);
    expect(screen.queryByText("Not connected")).toBeNull();
    expect(screen.getAllByRole("button", { name: "Add a venture" }).length).toBeGreaterThan(0);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Restore" })); });
    expect(w.archiveVenture).toHaveBeenCalledWith("shop", false);
  });
  it("a venture page can be edited and archived with Undo", async () => {
    const w = fake();
    const data = snap(ledger({ detail: { headline: "h", checklist: [], milestones: [], nextActions: [], budgetUsedJpy: null, fundJpy: null, openVotes: 0 } }));
    render(<BusinessField view="ventures/shop" data={data} adapter={liveBusiness(data.ledger)} writes={w} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Archive" })); });
    expect(w.archiveVenture).toHaveBeenCalledWith("shop", true);
    expect(screen.getByRole("status")).toHaveTextContent("Archived Shop.");
    fireEvent.click(screen.getByRole("button", { name: "Edit Shop" }));
    expect(screen.getByRole("form", { name: "Edit venture" })).toBeInTheDocument();
  });
  it("Finances: month by venture, a rate form behind one button, and a link to Treasury", async () => {
    const w = fake();
    const onOpen = vi.fn();
    const data = snap(ledger());
    render(<RevenueField data={data} adapter={liveBusiness(data.ledger)} writes={w} onOpen={onOpen} />);
    expect(screen.getByRole("list", { name: "This month by venture" })).toHaveTextContent("Shop");
    fireEvent.click(screen.getByRole("button", { name: "Set the rate" }));
    expect(screen.getByRole("form", { name: "Exchange rate" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Log income or a cost in Treasury/ }));
    expect(onOpen).toHaveBeenCalledWith({ id: "enterprise", view: "treasury" });
  });
  it("Tasks: the old wiring notes are gone and each open task has Mark done", () => {
    render(<BusinessField view="tasks" data={snap(null) && { ...snap(null), tasks: [{ title: "Order jars", status: "active", priority: "high", tags: [] }] }} writes={fake()} />);
    expect(document.body.textContent).not.toMatch(/venture link needs wiring|needs wiring; Amara updates/i);
    expect(screen.getByRole("button", { name: "Mark done: Order jars" })).toBeInTheDocument();
  });
});

describe("Enterprise History (ledger changes with Undo)", () => {
  const at = (ms: number) => new Date(ms).toISOString();
  const NOW = Date.parse("2026-11-25T03:00:00Z");
  const hist = (extra: LedgerChange[] = []): LedgerChange[] => [
    { id: "c3", at: at(NOW - 2000), action: "venture_edit", subject: "shop", label: "Shop", undone: false },
    { id: "c2", at: at(NOW - 3_600_000), action: "fx_set", subject: "fx", label: "150.25", undone: false },
    { id: "c1", at: at(NOW - 7_200_000), action: "venture_add", subject: "shop", label: "Shop", undone: true },
    ...extra,
  ];
  it("lists the last 10 changes in plain words; only the newest still-open one has Undo", async () => {
    const many = Array.from({ length: 12 }, (_, i) => ({ id: `x${i}`, at: at(NOW - 3_600_000 * (i + 5)), action: "venture_archive", subject: "a", label: `V${i}`, undone: false }));
    const w = fake({ history: vi.fn(async () => [...hist(), ...many]) });
    render(<LedgerHistory writes={w} now={() => NOW} />);
    const list = await screen.findByRole("list", { name: "Recent changes" });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(10));
    expect(list).toHaveTextContent("Edited: Shop");
    expect(list).toHaveTextContent("Yen rate set: 150.25");
    expect(list).toHaveTextContent("Added: Shop");
    expect(within(list).getByText("Undone")).toBeInTheDocument();
    expect(within(list).getAllByRole("button", { name: /^Undo/ })).toHaveLength(1);
    expect(changeText(hist()[0])).toBe("Edited: Shop");
  });
  it("Undo calls ledger undo for that change, says so, and reloads", async () => {
    const history = vi.fn(async () => hist());
    const w = fake({ history });
    render(<LedgerHistory writes={w} now={() => NOW} />);
    const undo = await screen.findByRole("button", { name: "Undo: Edited: Shop" });
    await act(async () => { fireEvent.click(undo); });
    expect(w.undoChange).toHaveBeenCalledWith("c3");
    expect(screen.getByRole("status")).toHaveTextContent("Undone.");
    await waitFor(() => expect(history.mock.calls.length).toBeGreaterThan(1));
  });
  it("no Undo once the 8 second window has passed, and a refused undo says why", async () => {
    const w = fake({ history: vi.fn(async () => hist()) });
    const { unmount } = render(<LedgerHistory writes={w} now={() => NOW + 9000} />);
    await screen.findByRole("list", { name: "Recent changes" });
    expect(screen.queryByRole("button", { name: /^Undo/ })).toBeNull();
    unmount();
    const w2 = fake({ history: vi.fn(async () => hist()), undoChange: vi.fn(async () => false) });
    render(<LedgerHistory writes={w2} now={() => NOW} />);
    const undo = await screen.findByRole("button", { name: /^Undo/ });
    await act(async () => { fireEvent.click(undo); });
    expect(screen.getByRole("status")).toHaveTextContent("can't be undone any more");
  });
  it("empty and unavailable states; nothing without a backend; and it shows on the start page", async () => {
    const { unmount } = render(<LedgerHistory writes={fake()} />);
    expect(await screen.findByText(/No changes yet/)).toBeInTheDocument();
    unmount();
    const { container, unmount: u2 } = render(<LedgerHistory writes={null} />);
    expect(container).toBeEmptyDOMElement();
    u2();
    render(<LedgerHistory writes={fake({ history: vi.fn(async () => { throw "x"; }) })} />);
    expect(await screen.findByText(/History isn.t available/)).toBeInTheDocument();
    const data = snap(ledger());
    const { unmount: u3 } = render(<BusinessField data={data} adapter={liveBusiness(data.ledger)} writes={fake({ history: vi.fn(async () => hist()) })} />);
    expect(await screen.findAllByRole("list", { name: "Recent changes" })).not.toHaveLength(0);
    u3();
  });
});

describe("Venture on tasks and content", () => {
  it("withVenture adds the venture's name to what the add reads, once, and leaves other methods alone", async () => {
    const parse = vi.fn(async (t: string) => ({ state: "live", value: { kind: "ask", question: t, options: [] } }));
    const base = { command: "x", wired: true, parse, save: vi.fn(), undo: vi.fn(), history: vi.fn() } as unknown as AddAdapter;
    const a = withVenture(base, "Shop");
    await a.parse("Order jars");
    await a.parse("order jars for shop");
    expect(parse.mock.calls.map((c) => c[0])).toEqual(["Order jars for Shop", "order jars for shop"]);
    expect(a.save).toBe(base.save);
    expect(a.wired).toBe(true);
    expect(withVenture(base, null)).toBe(base);
  });
  it("VenturePick offers No venture plus each venture, and nothing when there are none", () => {
    const change = vi.fn();
    const { container, unmount } = render(<VenturePick ventures={[{ id: "shop", name: "Shop" }]} value="" onChange={change} />);
    fireEvent.change(screen.getByLabelText("Venture (optional)"), { target: { value: "shop" } });
    expect(change).toHaveBeenCalledWith("shop");
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["No venture", "Shop"]);
    unmount();
    const r = render(<VenturePick ventures={[]} value="" onChange={change} />);
    expect(r.container).toBeEmptyDOMElement();
    expect(container).toBeDefined();
  });
  it("Tasks and Content rows show the venture name; the raw tag is not shown", () => {
    const data = { ...snap(ledger()), tasks: [{ title: "Order jars", status: "active", priority: null, tags: ["venture:shop", "launch"], venture: "shop" }], pipeline: [{ title: "Jar reel", stage: "ideation", content_type: "reel", notes: null, venture: "shop" }] };
    const { unmount } = render(<BusinessField view="tasks" data={data} adapter={liveBusiness(data.ledger)} writes={fake()} />);
    expect(screen.getByText(/Venture: Shop/)).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/venture:shop|Venture shop/i);
    expect(screen.getByLabelText("Venture (optional)")).toBeInTheDocument();
    unmount();
    render(<BusinessField view="content" data={data} adapter={liveBusiness(data.ledger)} writes={fake()} />);
    expect(screen.getByText(/Reel · Venture: Shop/)).toBeInTheDocument();
  });
});
