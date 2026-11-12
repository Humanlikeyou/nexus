import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CAUSAL } from "../takes/adapters";
import { MONEY_BANNED, NFA_LINE, SAMPLE_MONEY_TEXT, sampleMoney, unwiredMoney, type MoneyAdapter } from "./adapters";
import { ConnectSheet } from "./ConnectSheet";
import { MoneyAccounts, MoneyFollowUps, MoneyOverviewSection, MoneyTake } from "./MoneyView";
import { guardWatchInput } from "./seedGuard";

const PHRASE_12 = "abandon ability able about above absent absorb abstract absurd abuse access accident";
const PHRASE_24 = `${PHRASE_12} ${PHRASE_12}`;
const PHRASE_JA = "あいこくしん あいさつ あいだ あおぞら あかちゃん あきる あけがた あける あこがれる あさい あさひ あしあと";
const XPUB = `xpub6${"C".repeat(106)}`;

describe("design-1111t money: seed guard", () => {
  it("refuses recovery phrases (English, Japanese, 12 and 24 words) and private keys", () => {
    for (const s of [PHRASE_12, PHRASE_24, PHRASE_JA, `  ${PHRASE_12.toUpperCase()}\n`]) expect(guardWatchInput(s)).toEqual({ kind: "secret", why: "phrase" });
    expect(guardWatchInput(`xprv9${"s".repeat(106)}`)).toMatchObject({ kind: "secret" });
    expect(guardWatchInput(`5${"H".repeat(50)}`)).toMatchObject({ kind: "secret" });
    expect(guardWatchInput("a".repeat(64))).toMatchObject({ kind: "secret" });
    expect(guardWatchInput(`0x${"b".repeat(64)}`)).toMatchObject({ kind: "secret" });
  });
  it("accepts xpubs and public addresses only", () => {
    expect(guardWatchInput(XPUB)).toMatchObject({ kind: "public", form: "xpub", label: "Looks like a public key" });
    expect(guardWatchInput("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq")).toMatchObject({ kind: "public", chain: "btc" });
    expect(guardWatchInput(`0x${"a".repeat(40)}`)).toMatchObject({ kind: "public", chain: "eth" });
    expect(guardWatchInput("hello there")).toEqual({ kind: "invalid" });
  });
  it("a pasted phrase is cleared at once and never reaches the adapter, a log or the DOM", async () => {
    const add = vi.fn(async () => ({ state: "sample" as const, value: { ok: true as const } }));
    const adapter: MoneyAdapter = { ...sampleMoney(), addWatchOnly: add };
    const log = vi.spyOn(console, "log"); const warn = vi.spyOn(console, "warn"); const err = vi.spyOn(console, "error");
    const store = vi.spyOn(Storage.prototype, "setItem");
    const { container } = render(<ConnectSheet open onClose={() => {}} adapter={adapter} />);
    const input = screen.getByRole("textbox", { name: /xpub or address/ });
    fireEvent.change(input, { target: { value: PHRASE_24 } });
    expect(input).toHaveValue("");
    expect(screen.getByRole("alert")).toHaveTextContent(/cleared it and didn.t save it anywhere/);
    expect(container.innerHTML).not.toContain("abandon");
    expect(screen.getByRole("button", { name: "Add watch-only" })).toBeDisabled();
    for (const spy of [log, warn, err, store]) for (const call of spy.mock.calls) expect(JSON.stringify(call)).not.toContain("abandon");
    expect(add).not.toHaveBeenCalled();
    fireEvent.change(input, { target: { value: XPUB } });
    expect(screen.getByText("✓ Looks like a public key")).toBeInTheDocument();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add watch-only" })); });
    expect(add).toHaveBeenCalledWith("btc", XPUB, "Ledger · Bitcoin");
  });
});

describe("design-1111t money: read-only", () => {
  it("the adapter has no way to move money", () => {
    const keys = [...Object.keys(unwiredMoney), ...Object.keys(sampleMoney())];
    for (const k of keys) expect(k).not.toMatch(/transfer|pay(?!day)|send|trade|buy|sell|convert|withdraw/i);
  });
  it("sample copy has no banned or causal words, and the take ends with Not financial advice", () => {
    for (const s of SAMPLE_MONEY_TEXT) { expect(s).not.toMatch(MONEY_BANNED); expect(s).not.toMatch(CAUSAL); }
    render(<MoneyTake adapter={sampleMoney()} />);
    expect(screen.getByText(NFA_LINE)).toBeInTheDocument();
    expect(screen.getByText(/Risk:/).parentElement).toHaveTextContent(/fall 30%/);
    expect(screen.getAllByText("Placeholder citation · not checked yet").length).toBe(2);
  });
  it("every account shows Read-only or Watch-only and Can't move money; no account numbers", () => {
    render(<MoneyAccounts adapter={sampleMoney()} />);
    expect(screen.getAllByText("Can't move money")).toHaveLength(8);
    expect(screen.getAllByText(/Read-only$/)).toHaveLength(6);
    expect(screen.getAllByText(/Watch-only$/)).toHaveLength(2);
    expect(document.body.textContent).not.toMatch(/\d{6,}(?![\d,])/);
  });
  it("USD with a JPY view; venture funds stay out of the total", () => {
    const { rerender } = render(<MoneyOverviewSection adapter={sampleMoney()} currency="USD" onCurrency={() => {}} />);
    expect(screen.getByText("$20,607")).toBeInTheDocument();
    expect(screen.getByText(/Venture funds · not in your personal total/)).toBeInTheDocument();
    rerender(<MoneyOverviewSection adapter={sampleMoney()} currency="JPY" onCurrency={() => {}} />);
    expect(screen.getByText("¥3,115,778")).toBeInTheDocument();
    expect(screen.getAllByText("Sample data").length).toBeGreaterThan(0);
  });
  it("live overview: no Sample tag, buffer and savings rate stay Needs wiring, no USD without fx", () => {
    const overview = { totalUsd: null, balances: 0, fx: null, cash: { usd: null, note: "300,000 JPY · no USD value until an FX rate is stored" }, investments: { usd: null, note: "No investment balance entered" }, crypto: { usd: null, note: "No crypto balance entered" }, bufferMonths: null, bufferNote: "n", allocation: [], rows: [{ name: "SMBC main", native: "300,000 JPY", usd: null, d30: "–" }], venture: [], savingsRate: null };
    const adapter: MoneyAdapter = { ...unwiredMoney, overview: () => ({ state: "live", value: overview }) };
    render(<MoneyOverviewSection adapter={adapter} currency="USD" onCurrency={() => {}} />);
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThanOrEqual(5);
    expect(screen.getByText("300,000 JPY")).toBeInTheDocument();
    expect(screen.queryByText(/Venture funds/)).toBeNull();
    expect(screen.queryByText(/\$\d/)).toBeNull();
  });
  it("states: expired is greyed with as-of and no suggestion on partial data", () => {
    render(<><MoneyAccounts adapter={sampleMoney({ scenario: "states" })} /><MoneyTake adapter={sampleMoney({ scenario: "states" })} /></>);
    expect(screen.getByText("as of Oct 19")).toBeInTheDocument();
    expect(screen.getByText("No money suggestion today.")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: /Your answer/ })).toBeNull();
  });
  it("unwired: Needs wiring everywhere, no numbers, no Sample data", () => {
    render(<><MoneyAccounts adapter={unwiredMoney} /><MoneyOverviewSection adapter={unwiredMoney} currency="USD" onCurrency={() => {}} /><MoneyTake adapter={unwiredMoney} /><MoneyFollowUps adapter={unwiredMoney} /></>);
    expect(screen.getAllByText("Needs wiring").length).toBeGreaterThan(0);
    expect(screen.queryByText("Sample data")).toBeNull();
    expect(screen.queryByText(/\$\d/)).toBeNull();
  });
  it("follow-ups: returns are not judged, FX is can't tell by design", () => {
    render(<MoneyFollowUps adapter={sampleMoney()} />);
    expect(screen.getByText(/Too short to mean anything/)).toBeInTheDocument();
    expect(screen.getByText(/Can't tell, by design/)).toBeInTheDocument();
  });
});

describe("live account rows", () => {
  const row = (o: object) => ({ id: "x", nickname: "Row", institution: "ledger", route: "watch_only", region: "crypto", via: "v", status: "ok", synced: "s", asOf: null, native: "0.5 BTC", usd: null, currency: "BTC", scope: "personal", keychain: "not_set", ...o });
  it("shows the native amount, no USD tag for crypto or a missing balance, tag only for JPY without a rate", () => {
    const mk = (a: object): MoneyAdapter => ({ ...unwiredMoney, accounts: () => ({ state: "live", value: [row(a) as never] }) });
    const { container, rerender } = render(<MoneyAccounts adapter={mk({})} />);
    const amt = () => container.querySelector(".m-acct__amt")?.textContent ?? "";
    expect(amt()).toBe("0.5 BTC");
    rerender(<MoneyAccounts adapter={mk({ currency: "JPY", region: "jp", native: "–" })} />);
    expect(amt()).toBe("–");
    rerender(<MoneyAccounts adapter={mk({ currency: "JPY", region: "jp", native: "300,000 JPY" })} />);
    expect(amt()).toMatch(/USD/);
  });
});
