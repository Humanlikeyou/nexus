import { sample, sampleAdaptersEnabled, UNWIRED, type Wired } from "../lib/sampleAdapters";
import type { TakeNote } from "../amaraTake/takeView";
import type { Action, AnswerState, SkipReason, Suggestion } from "../takes/adapters";
import { inTauri } from "../lib/localStores";
import { makeLiveMoney } from "./live";

/** design-1111t §9: Business › Personal money. READ-ONLY / WATCH-ONLY. There is no method here that moves, pays,
 *  trades or converts anything, and none may be added (see money.test.ts). Secrets never pass through this layer:
 *  the UI only ever sees `keychain: "set" | "not_set"`. */

export type Institution = "chime" | "fidelity" | "jp_bank" | "ledger";
export type Route = "aggregator" | "csv" | "bank_api" | "watch_only" | "manual";
export type AccountStatus = "ok" | "stale" | "expired" | "error" | "not_connected";
export type Region = "us" | "jp" | "crypto";

export type MoneyAccount = {
  id: string; nickname: string; institution: Institution; route: Route; region: Region; via: string;
  status: AccountStatus; synced: string; asOf: string | null;
  /** Native display, e.g. "$3,412", "¥612,000", "0.041 BTC". */
  native: string; usd: number | null; currency: "USD" | "JPY" | "BTC" | "ETH";
  scope: "personal" | "venture"; keychain: "set" | "not_set"; note?: string;
};

export type Fx = { rate: number; at: string; source: string };
export type MoneyRow = { name: string; native: string; usd: number | null; d30: string; d30note?: string };
/** A null amount is "unknown" (no balance, no FX rate, or no price source), never 0. */
export type MoneyOverview = {
  totalUsd: number | null; balances: number; fx: Fx | null;
  cash: { usd: number | null; note: string }; investments: { usd: number | null; note: string }; crypto: { usd: number | null; note: string };
  bufferMonths: number | null; bufferNote: string;
  allocation: Array<{ label: string; pct: number; tone: "a" | "b" | "c" | "d" }>;
  rows: MoneyRow[];
  venture: Array<{ name: string; sub: string; usd: number | null }>;
  /** Null when income and spend aren't available (the live adapter has neither). */
  savingsRate: { pct: number; delta: string; note: string; month: string } | null;
};

export type MoneyVerdict = "locked_in" | "better" | "not_judged" | "unclear";
export type MoneyFollowUp = { id: string; group: string; verdict: MoneyVerdict; chip: string; title: string; body: string; rows: Array<{ label: string; value: string; note: string }> };

export type MoneyTake =
  | { kind: "suggestion"; note: TakeNote; memory: string | null; suggestion: Suggestion; historyCount: number }
  | { kind: "partial"; note: TakeNote; why: string; historyCount: number };

export type ConnectRoute = { id: Institution; name: string; mono: string; sub: string; ready: Wired<{ ok: true }> };

export interface MoneyAdapter {
  accounts(): Wired<MoneyAccount[]>;
  /** The one JPY-per-USD reference rate with its own date and source. Unwired when none is stored; never a guess. */
  fx(): Wired<Fx>;
  overview(): Wired<MoneyOverview>;
  take(): Wired<MoneyTake>;
  followUps(): Wired<MoneyFollowUp[]>;
  routes(): ConnectRoute[];
  /** Adds a watch-only address or xpub. Only ever called with a value guardWatchInput() accepted as public. */
  addWatchOnly(chain: "btc" | "eth", publicValue: string, nickname: string, balance?: number | null): Promise<Action>;
  /** Opens the native file dialog (the renderer never passes a path) and imports the statement. Live text comes from connectNote(). */
  importStatement(): Promise<Action>;
  /** The fixed sentence for the last connect action's result (counts or a fixed error text); null before any. */
  connectNote(): string | null;
  answer(id: string, state: AnswerState, reason?: SkipReason | null): Promise<Action>;
  undo(id: string): Promise<Action>;
  reconnect(id: string): Promise<Action>;
  disconnect(id: string): Promise<Action>;
}

/** Fixed footer on every money take (§9.4). */
export const NFA_LINE = "Not financial advice. Amara reads your accounts and suggests; she can't move, buy, sell or convert anything, and you decide.";
/** §9.4 validator additions: money copy may never contain these. */
export const MONEY_BANNED = /\b(guarantee\w*|risk-free|sure thing|will go up|moon|get rich)\b/i;
/** Sample reference rate only (README §7): ¥151.20 = $1. Never shown without the Sample data tag. */
export const SAMPLE_FX: Fx = { rate: 151.2, at: "Oct 28, 9:00 AM", source: "sample reference rate" };

const ROUTES_UNWIRED: ConnectRoute[] = [
  { id: "chime", name: "Chime", mono: "C", sub: "Read-only aggregator sign-in", ready: UNWIRED },
  { id: "fidelity", name: "Fidelity", mono: "F", sub: "Read-only aggregator sign-in", ready: UNWIRED },
  { id: "jp_bank", name: "Japanese bank", mono: "銀", sub: "Statement CSV import · bank API if one exists", ready: UNWIRED },
  { id: "ledger", name: "Ledger (watch-only)", mono: "₿", sub: "Public address or xpub", ready: UNWIRED },
];

export const unwiredMoney: MoneyAdapter = {
  accounts: () => UNWIRED, fx: () => UNWIRED, overview: () => UNWIRED, take: () => UNWIRED, followUps: () => UNWIRED,
  routes: () => ROUTES_UNWIRED,
  connectNote: () => null,
  addWatchOnly: async () => UNWIRED, importStatement: async () => UNWIRED, answer: async () => UNWIRED, undo: async () => UNWIRED, reconnect: async () => UNWIRED, disconnect: async () => UNWIRED,
};

// ---- Sample fixtures (mock m1–m6 numbers; sample mode only, always tagged) ----
const A = (a: Omit<MoneyAccount, "scope" | "keychain" | "asOf"> & Partial<Pick<MoneyAccount, "asOf" | "keychain">>): MoneyAccount => ({ scope: "personal", keychain: a.route === "aggregator" ? "set" : "not_set", asOf: null, ...a });
const ACCOUNTS: MoneyAccount[] = [
  A({ id: "chime-chk", nickname: "Chime · Checking", institution: "chime", route: "aggregator", region: "us", via: "Via a read-only aggregator · balances + transactions", status: "ok", synced: "Synced 7:02 AM", native: "$3,412", usd: 3412, currency: "USD" }),
  A({ id: "chime-sav", nickname: "Chime · Savings", institution: "chime", route: "aggregator", region: "us", via: "Via a read-only aggregator", status: "ok", synced: "Synced 7:02 AM", native: "$4,880", usd: 4880, currency: "USD" }),
  A({ id: "fid-brk", nickname: "Fidelity · Individual brokerage", institution: "fidelity", route: "aggregator", region: "us", via: "Via a read-only aggregator · holdings + balances", status: "ok", synced: "Synced 6:40 AM", native: "$14,930", usd: 14930, currency: "USD" }),
  A({ id: "fid-ira", nickname: "Fidelity · Roth IRA", institution: "fidelity", route: "aggregator", region: "us", via: "Via a read-only aggregator · holdings", status: "ok", synced: "Synced 6:40 AM", native: "$9,610", usd: 9610, currency: "USD" }),
  A({ id: "jp1", nickname: "Japan bank 1 · Ordinary (普通)", institution: "jp_bank", route: "csv", region: "jp", via: "Statement import (CSV) · you add a file each month", status: "ok", synced: "Imported Oct 25", native: "¥612,000", usd: 4048, currency: "JPY" }),
  A({ id: "jp2", nickname: "Japan bank 2 · Time deposit (定期)", institution: "jp_bank", route: "csv", region: "jp", via: "Statement import (CSV)", status: "ok", synced: "Imported Oct 25", native: "¥1,250,000", usd: 8267, currency: "JPY" }),
  A({ id: "ledger-btc", nickname: "Ledger · Bitcoin", institution: "ledger", route: "watch_only", region: "crypto", via: "Watch-only · 1 extended public key (xpub) · addresses only", status: "ok", synced: "Checked 7:10 AM", native: "0.041 BTC", usd: 2870, currency: "BTC" }),
  A({ id: "ledger-eth", nickname: "Ledger · Ethereum", institution: "ledger", route: "watch_only", region: "crypto", via: "Watch-only · 1 public address", status: "ok", synced: "Checked 7:10 AM", native: "0.52 ETH", usd: 1664, currency: "ETH" }),
];
/** m6: the same rows when a connection is missing, old or broken. */
const ACCOUNTS_STATES: MoneyAccount[] = [
  { ...ACCOUNTS[0], status: "expired", synced: "Last synced Oct 19 · 9 days ago", native: "$3,190", usd: 3190, asOf: "Oct 19", note: "Read access ended on Oct 19 (aggregators ask every 90 days). Left out of today's suggestion." },
  { ...ACCOUNTS[2], status: "not_connected", synced: "Not connected", native: "–", usd: null, keychain: "not_set" },
  { ...ACCOUNTS[4], status: "stale", synced: "34 days old", native: "¥572,000", usd: null, asOf: "Sep 24", note: "The newest statement is 34 days old. Drop October's CSV to update." },
  { ...ACCOUNTS[6], usd: null, note: "Price feed not wired, so there's no USD value and crypto is left out of the total." },
];

const OVERVIEW: MoneyOverview = {
  totalUsd: 49681, balances: 8, fx: SAMPLE_FX,
  cash: { usd: 20607, note: "Chime $8,292 · Japan ¥1,862,000 ≈ $12,315" },
  investments: { usd: 24540, note: "Fidelity brokerage + Roth IRA" },
  crypto: { usd: 4534, note: "9% of personal · BTC + ETH" },
  bufferMonths: 5.5, bufferNote: "Savings $13,147 ÷ usual spend $2,390/mo",
  allocation: [{ label: "Cash USD", pct: 17, tone: "a" }, { label: "Cash JPY", pct: 25, tone: "b" }, { label: "Investments", pct: 49, tone: "c" }, { label: "Crypto", pct: 9, tone: "d" }],
  rows: [
    { name: "Chime · Checking + Savings", native: "$8,292", usd: 8292, d30: "+$410" },
    { name: "Japan banks · Ordinary + Time deposit", native: "¥1,862,000", usd: 12315, d30: "+¥40,000", d30note: "FX moved it −$96" },
    { name: "Fidelity · Brokerage + Roth IRA", native: "$24,540", usd: 24540, d30: "+$612", d30note: "$250 added, rest market" },
    { name: "Ledger · BTC + ETH (watch-only)", native: "0.041 BTC · 0.52 ETH", usd: 4534, d30: "−$188", d30note: "price only" },
  ],
  venture: [{ name: "Taiyo Skin operating account", sub: "Business › Ventures", usd: 6240 }, { name: "Trading bankroll", sub: "Business › Money · risk index 0.18", usd: 3000 }, { name: "Consulting receivable", sub: "Due Nov 6", usd: 1820 }],
  savingsRate: { pct: 14, delta: "▲ 3 pts vs your 6-month average 11%", note: "Income $3,900 (personal draws + salary) · saved or invested $546.", month: "October" },
};

const TAKE_NOTE: TakeNote = {
  text: "Your emergency buffer is **5.5 months**, inside the **3–6** most guides aim for. Chime checking has stayed above **$3,000** for **3 months**, and your savings rate was **14%** in October.",
  chips: [{ label: "Buffer", value: "5.5 months" }, { label: "Checking", value: "$3,412" }, { label: "Savings rate", value: "14%" }, { label: "Fees now", value: "$41 / yr" }],
  time: "Today, 7:40", sourceLine: "From 8 read-only balances, 6 months of transactions and your Fidelity holdings", question: "What's behind today's money note?",
};

export const MONEY_SUGGESTION: Suggestion = {
  id: "s-money-1028", page: "business", type: "payday_index_transfer", typeLabel: "Payday transfer into a fund you hold", seen: "Habit check · 3 paydays",
  title: "Set a $250 automatic transfer on payday into the broad index fund you already hold at Fidelity.",
  how: "You set it up in Fidelity, not Amara. Start on the Nov 15 payday. Your 5.5-month buffer stays where it is.",
  why: "Your buffer is covered, so new savings can go to long-term money. Your payday auto-transfer to Chime savings took your savings rate from **9%** to **14%** (Aug–Oct); this repeats that for money you won't need for 5+ years.",
  facts: [{ label: "Emergency buffer", value: "5.5 mo", note: "goal 3–6 mo" }, { label: "Chime checking", value: "$3,412", note: "above $3,000 for 3 mo" }, { label: "Fund cost", value: "0.03%/yr", note: "after Oct 16 switch" }],
  evidence: {
    level: "strong",
    summary: "Automatic, scheduled saving raised savings rates in large field studies. Over 15 years most actively managed US stock funds trailed low-cost index funds.",
    sources: [{ cite: "Thaler & Benartzi 2004 (Save More Tomorrow)", verified: false }, { cite: "S&P SPIVA US scorecards", verified: false }],
    risk: "Index funds can fall 30% or more in a bad year and may take years to recover. Only invest money you won't need for 5+ years.",
    alsoConsidered: "Timing the yen–dollar rate (no one predicts it reliably; if you need dollars, convert a set amount monthly instead) · crypto top-ups (not suggested: already 9% of personal and highly volatile).",
  },
  detectRule: "Counts by itself when a recurring $250 deposit shows in Fidelity.",
  answer: { state: "waiting" },
};

const MEMORY = "**Oct 14** I flagged **$186 a year** in fund fees (a 0.75% fund). You switched on **Oct 16**; estimated fees are now **$41 a year**. Fees are set in advance, so that saving is close to certain, unlike returns.";

const FOLLOWUPS: MoneyFollowUp[] = [
  { id: "fees", group: "Locked in · fees, Oct 16", verdict: "locked_in", chip: "Saving locked in", title: "Switch from a 0.75% fund to a 0.03% index fund.", body: "Estimated fees fell from **$186** to **$41** a year on **$24,540**. Fees are known in advance, so this one is close to certain.",
    rows: [{ label: "Fees before", value: "$186 / yr", note: "0.75%" }, { label: "Fees now", value: "$41 / yr", note: "blended" }, { label: "Difference", value: "$145 / yr", note: "estimate" }] },
  { id: "rate", group: "Looks better · savings rate, Oct 31", verdict: "better", chip: "Looks better · your habit", title: "Auto-transfer $200 to Chime savings on payday, Aug–Oct.", body: "Saved **14%** a month on average over 3 paydays, vs **9%** in the 3 months before. Income was similar (**$3,900** vs **$3,850**), so it's a fair comparison, but 3 months is short.",
    rows: [{ label: "Savings rate · with", value: "14%", note: "3 months" }, { label: "Before", value: "9%", note: "3 months" }, { label: "Income", value: "$3,900 vs $3,850", note: "checked" }] },
  { id: "returns", group: "Not judged · returns", verdict: "not_judged", chip: "Too short to mean anything", title: "Fidelity is up $362 this month apart from what you added.", body: "One month of market moves says nothing about whether a plan is good. Amara shows it and never calls it a win or a loss. She looks at 3- and 5-year results only.",
    rows: [{ label: "Market change", value: "+$362", note: "30 days" }, { label: "You added", value: "$250", note: "deposits" }, { label: "Judged at", value: "3 + 5 yrs", note: "only" }] },
  { id: "fx", group: "Can't tell · yen to dollars", verdict: "unclear", chip: "Can't tell, by design", title: "Convert ¥100,000 a month instead of waiting for a better rate.", body: "Your average rate was **¥149.80**; today's is **¥151.20**. That gap is luck, not skill, either way. Amara rates the habit (a set amount, low fees), not the rate you got.",
    rows: [{ label: "Your average", value: "¥149.80", note: "3 conversions" }, { label: "Today", value: "¥151.20", note: "reference" }, { label: "Transfer fees", value: "¥1,980", note: "3 months" }] },
];

export type MoneyPreview = { scenario?: "connected" | "states" | "none" };
export const MONEY_PREVIEW_KEY = "nexus.moneyPreview";

export function sampleMoney(opts: MoneyPreview = {}): MoneyAdapter {
  const scenario = opts.scenario ?? "connected";
  const ok = async (): Promise<Action> => sample({ ok: true });
  return {
    connectNote: () => null,
    accounts: () => (scenario === "none" ? sample([]) : sample(scenario === "states" ? ACCOUNTS_STATES : ACCOUNTS)),
    fx: () => sample(SAMPLE_FX),
    overview: () => (scenario === "connected" ? sample(OVERVIEW) : UNWIRED),
    take: () => (scenario === "none" ? UNWIRED : sample(scenario === "states"
      ? { kind: "partial", note: { ...TAKE_NOTE, text: "Chime needs you to sign in again, so today's numbers are partial.", chips: [] }, why: "Chime is expired, so Amara doesn't have a full picture. She won't advise on half the data. Sign in to Chime again and tomorrow's take will include it.", historyCount: 22 }
      : { kind: "suggestion", note: TAKE_NOTE, memory: MEMORY, suggestion: MONEY_SUGGESTION, historyCount: 22 })),
    followUps: () => (scenario === "connected" ? sample(FOLLOWUPS) : UNWIRED),
    routes: () => ROUTES_UNWIRED.map((r) => (r.id === "ledger" || r.id === "jp_bank" ? { ...r, ready: sample({ ok: true as const }) } : r)),
    addWatchOnly: ok, importStatement: ok, answer: ok, undo: ok, reconnect: ok, disconnect: ok,
  };
}

function readPreview(): MoneyPreview {
  try { const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(MONEY_PREVIEW_KEY); return raw ? (JSON.parse(raw) as MoneyPreview) : {}; } catch { return {}; }
}

let cached: MoneyAdapter | null = null;
let liveCached: MoneyAdapter | null = null;
/** Sample when sample mode is on (localStorage nexus.sampleAdapters=1); the live read-only adapter inside Tauri; otherwise unwired. */
export function moneyAdapter(): MoneyAdapter {
  if (sampleAdaptersEnabled()) return (cached ??= sampleMoney(readPreview()));
  if (inTauri()) return (liveCached ??= makeLiveMoney());
  return unwiredMoney;
}
export function resetMoneyAdapter() { cached = null; liveCached = null; }
export const SAMPLE_MONEY_TEXT = [TAKE_NOTE.text, MEMORY, MONEY_SUGGESTION.title, MONEY_SUGGESTION.how, MONEY_SUGGESTION.why, MONEY_SUGGESTION.evidence.summary, MONEY_SUGGESTION.evidence.risk ?? "", MONEY_SUGGESTION.evidence.alsoConsidered ?? "", ...FOLLOWUPS.flatMap((f) => [f.title, f.body])];
