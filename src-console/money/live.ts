import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { announceStores, live, realInvoke, type Invoke } from "../lib/localStores";
import { guardWatchInput } from "./seedGuard";
import { unwiredMoney, type ConnectRoute, type Fx, type Institution, type MoneyAccount, type MoneyAdapter, type MoneyOverview, type Region, type Route } from "./adapters";
import { buildOverview, type RawBalance } from "./overview";

/**
 * Live money adapter over the READ-ONLY Rust commands `money_accounts_list` and `fx_rate_get`
 * (src-tauri/src/commands/money.rs). Reads are synchronous over a cache; a store is wired only after its
 * first successful read, so a missing backend keeps showing "Needs wiring". Nothing here invents a value:
 * account rows still show no balance (only the overview reads balances), so every account balance stays "–" (not 0) with no USD value, and a
 * null FX rate is unwired. `money.overview` is built from accounts + `money_balances_list` (see overview.ts). `fx_rate_refresh` is never called here (it is an outbound fetch Taisun triggers).
 */

type RawAccount = { id: string; nickname: string; institution: string; kind: string; route: string; currency: string; scope: string; createdAt?: string; created_at?: string };
type RawBalances = { balances: RawBalance[]; fx: RawFx | null };
type RawFx = { rate: number; date: string; source: string; fetchedAt?: string; fetched_at?: string };

const INSTITUTION: Record<string, Institution> = { chime: "chime", fidelity: "fidelity", ledger: "ledger", smbc: "jp_bank", japan_post: "jp_bank" };
const ROUTE: Record<string, Route> = { csv: "csv", watch_only: "watch_only", manual: "manual" };
const CURRENCY: Record<string, MoneyAccount["currency"]> = { USD: "USD", JPY: "JPY", BTC: "BTC", ETH: "ETH" };
const REGION: Record<MoneyAccount["currency"], Region> = { USD: "us", JPY: "jp", BTC: "crypto", ETH: "crypto" };
const VIA: Record<string, string> = { csv: "Statement import (CSV)", watch_only: "Watch-only · public address or key only", manual: "Balance you enter yourself" };

const words = (s: string) => s.replace(/_/g, " ");

/** Maps one stored account. Unknown enum values return null so a row is dropped rather than guessed. */
export function mapAccount(r: RawAccount): MoneyAccount | null {
  const institution = INSTITUTION[r.institution];
  const route = ROUTE[r.route];
  const currency = CURRENCY[r.currency];
  if (!institution || !route || !currency || (r.scope !== "personal" && r.scope !== "venture") || typeof r.id !== "string" || typeof r.nickname !== "string") return null;
  const added = (r.createdAt ?? r.created_at ?? "").slice(0, 10);
  return {
    id: r.id, nickname: r.nickname, institution, route, region: REGION[currency],
    via: `${VIA[r.route]} · ${words(r.kind)}`,
    status: "ok", synced: added ? `Added ${added}` : "Added",
    asOf: null,
    // The command returns no balances, so there is no amount to show and no USD value (never 0).
    native: "–", usd: null, currency, scope: r.scope, keychain: "not_set",
  };
}

const FX_SOURCE: Record<string, string> = { ecb: "ECB reference rate", manual: "entered by you" };

/** Maps the stored reference rate; a missing, non-positive or non-finite rate is unwired. */
export function mapFx(r: RawFx | null | undefined): Fx | null {
  if (!r || typeof r.rate !== "number" || !Number.isFinite(r.rate) || r.rate <= 0 || typeof r.date !== "string" || !r.date) return null;
  return { rate: r.rate, at: r.date, source: FX_SOURCE[r.source] ?? r.source };
}

export function makeLiveMoney(call: Invoke = realInvoke, notify: () => void = announceStores): MoneyAdapter {
  let accounts: MoneyAccount[] | null = null;
  let fx: Fx | null = null;
  let fxRead = false;
  let rawAccounts: RawAccount[] = [];
  let overview: MoneyOverview | null = null;
  let overviewRead = false;
  let started = false;
  let inflight: Promise<void> | null = null;

  const readAccounts = async () => { rawAccounts = await call<RawAccount[]>("money_accounts_list"); accounts = rawAccounts.map(mapAccount).filter((a): a is MoneyAccount => a !== null); };
  const readFx = async () => { fx = mapFx(await call<RawFx | null>("fx_rate_get")); fxRead = true; };
  // Read-only: balances come from money_balances_list (manual balances + statement running balances).
  let balances: RawBalances | null = null;
  const readBalances = async () => { balances = await call<RawBalances>("money_balances_list"); };
  function refresh(): Promise<void> {
    inflight ??= Promise.allSettled([readAccounts(), readFx(), readBalances()]).then(() => {
      overview = accounts && balances && Array.isArray(balances.balances) ? buildOverview(rawAccounts.filter((r) => mapAccount(r) !== null), balances.balances, mapFx(balances.fx)) : null;
      overviewRead = true;
    }).finally(() => { inflight = null; notify(); });
    return inflight;
  }
  const start = () => { if (!started) { started = true; void refresh(); } };

  const ledgerReady = { state: "live", value: { ok: true } } as const;
  return {
    ...unwiredMoney,
    // Ledger watch-only is wired. jp_bank stays unwired: money_import_statement needs an account id and a
    // file path, and importStatement() carries neither (no file picker is reachable from here).
    routes: (): ConnectRoute[] => unwiredMoney.routes().map((r) => (r.id === "ledger" ? { ...r, ready: ledgerReady } : r)),
    addWatchOnly: async (chain, publicValue, nickname) => {
      // The seed guard runs first, again, so a refused value never reaches invoke.
      const g = guardWatchInput(publicValue);
      if (g.kind !== "public" || g.chain !== chain) return UNWIRED;
      try {
        const acct = await call<{ id: string }>("money_account_add", { draft: { nickname: nickname.trim() || "Ledger", institution: "ledger", kind: chain, route: "watch_only", currency: chain === "btc" ? "BTC" : "ETH", scope: "personal" } });
        await call("money_watch_add", { accountId: acct.id, value: publicValue.trim() });
      } catch (e) { console.warn("[money] watch-only add refused:", typeof e === "string" ? e.slice(0, 40) : "failed"); return UNWIRED; }
      await refresh();
      return live({ ok: true as const });
    },
    accounts: (): Wired<MoneyAccount[]> => { start(); return accounts ? live(accounts) : UNWIRED; },
    overview: (): Wired<MoneyOverview> => { start(); return overviewRead && overview ? live(overview) : UNWIRED; },
    fx: (): Wired<Fx> => { start(); return fxRead && fx ? live(fx) : UNWIRED; },
  };
}
