import type { Fx, MoneyOverview, MoneyRow } from "./adapters";

/**
 * Builds the Personal money overview from stored accounts, `money_balances_list` and the stored FX rate.
 * Pure: no invoke, no clock. Nothing is invented: a missing balance is "–", a JPY amount has no USD value
 * without an FX rate, crypto has no price source (so no USD), and buffer months and savings rate stay null
 * because they need monthly spend and income, which no store holds yet.
 */

export type RawAccountLite = { id: string; nickname: string; kind: string; currency: string; scope: string };
export type RawBalance = { account_id?: string; accountId?: string; currency: string; amount: number; as_of?: string; asOf?: string; source: string; history?: Array<{ date: string; amount: number }> };

const CASH_KINDS = new Set(["checking", "savings", "ordinary", "time_deposit"]);
const INVEST_KINDS = new Set(["brokerage", "ira"]);

type Amt = { amount: number; currency: string };

/** Native amount text. Yen is written "612,000 JPY" (never the ¥ sign) so no yen figure looks converted. */
export function formatNative(n: number, currency: string): string {
  const sign = n < 0 ? "-" : "";
  if (currency === "USD") return `${sign}$${Math.abs(n).toLocaleString("en-US", { maximumFractionDigits: 2 })}`;
  return `${sign}${Math.abs(n).toLocaleString("en-US", { maximumFractionDigits: 8 })} ${currency}`;
}
const signed = (n: number, currency: string) => (n > 0 ? `+${formatNative(n, currency)}` : formatNative(n, currency));

function toUsd(amount: number, currency: string, fx: Fx | null): number | null {
  if (currency === "USD") return amount;
  if (currency === "JPY" && fx) return amount / fx.rate;
  return null; // BTC / ETH: no price source; JPY without a rate
}

/** Null when the list is empty or any item can't be priced (never a partial sum presented as a total). */
function sumUsd(items: Amt[], fx: Fx | null): number | null {
  if (items.length === 0) return null;
  let t = 0;
  for (const i of items) { const u = toUsd(i.amount, i.currency, fx); if (u == null) return null; t += u; }
  return t;
}

const nativeList = (items: Amt[]) => {
  const by = new Map<string, number>();
  for (const i of items) by.set(i.currency, (by.get(i.currency) ?? 0) + i.amount);
  return [...by].map(([c, a]) => formatNative(a, c)).join(" · ");
};

export function buildOverview(accounts: RawAccountLite[], balances: RawBalance[], fx: Fx | null): MoneyOverview | null {
  const bal = new Map<string, RawBalance>();
  for (const b of balances) { const id = b.account_id ?? b.accountId; if (id && Number.isFinite(b.amount)) bal.set(id, b); }
  const personal = accounts.filter((a) => a.scope === "personal");
  const venture = accounts.filter((a) => a.scope === "venture");
  const withBal = personal.filter((a) => bal.has(a.id));
  if (withBal.length === 0) return null; // nothing real to summarise: stays "Needs wiring"

  const item = (a: RawAccountLite): Amt => ({ amount: bal.get(a.id)!.amount, currency: a.currency });
  const crypto = withBal.filter((a) => a.currency === "BTC" || a.currency === "ETH" || a.kind === "btc" || a.kind === "eth");
  const isCrypto = new Set(crypto.map((a) => a.id));
  const cash = withBal.filter((a) => !isCrypto.has(a.id) && CASH_KINDS.has(a.kind)).map(item);
  const invest = withBal.filter((a) => !isCrypto.has(a.id) && INVEST_KINDS.has(a.kind)).map(item);
  const noBalance = personal.length - withBal.length;
  const missing = noBalance > 0 ? ` · ${noBalance} account${noBalance === 1 ? "" : "s"} with no balance` : "";

  const priced = [...cash, ...invest];
  const totalUsd = sumUsd(priced, fx);
  const tileNote = (list: Amt[], empty: string) =>
    list.length === 0 ? empty : `${nativeList(list)}${!fx && list.some((i) => i.currency === "JPY") ? " · no USD value until an FX rate is stored" : ""}`;

  const rows: MoneyRow[] = personal.map((a) => {
    const b = bal.get(a.id);
    if (!b) return { name: a.nickname, native: "–", usd: null, d30: "–" };
    const first = (b.history ?? []).find((p) => Number.isFinite(p.amount));
    const asOf = b.as_of ?? b.asOf ?? "";
    const hasDelta = !!first && !!asOf && first.date < asOf;
    return {
      name: a.nickname, native: formatNative(b.amount, a.currency), usd: toUsd(b.amount, a.currency, fx),
      d30: hasDelta ? signed(b.amount - first.amount, a.currency) : "–",
      d30note: hasDelta ? `since ${first.date}` : undefined,
    };
  });

  // Shares only when every balance is priced (no crypto, no unconverted yen); otherwise they would mislead.
  const parts: Array<{ label: string; usd: number | null; tone: "a" | "b" | "c" }> = [
    { label: "Cash USD", usd: sumUsd(cash.filter((i) => i.currency === "USD"), fx), tone: "a" },
    { label: "Cash JPY", usd: sumUsd(cash.filter((i) => i.currency === "JPY"), fx), tone: "b" },
    { label: "Investments", usd: sumUsd(invest, fx), tone: "c" },
  ];
  const allocation = totalUsd != null && totalUsd > 0 && crypto.length === 0
    ? parts.filter((p): p is { label: string; usd: number; tone: "a" | "b" | "c" } => p.usd != null && p.usd > 0).map((p) => ({ label: p.label, pct: Math.round((p.usd / totalUsd) * 100), tone: p.tone }))
    : [];

  const ventureRows = venture.filter((a) => bal.has(a.id)).map((a) => {
    const i = item(a);
    return { name: a.nickname, sub: formatNative(i.amount, i.currency), usd: toUsd(i.amount, i.currency, fx) };
  });

  return {
    totalUsd, balances: priced.length, fx,
    cash: { usd: sumUsd(cash, fx), note: tileNote(cash, "No cash balance entered") + missing },
    investments: { usd: sumUsd(invest, fx), note: tileNote(invest, "No investment balance entered") },
    crypto: { usd: null, note: crypto.length ? `${nativeList(crypto.map(item))} · watch-only, no price source so no USD value` : "No crypto balance entered" },
    bufferMonths: null, bufferNote: "Needs your usual monthly spend; no spending data yet",
    allocation, rows, venture: ventureRows, savingsRate: null,
  };
}
