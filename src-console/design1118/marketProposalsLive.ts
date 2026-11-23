import { invokeBound } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { announceStores, live } from "../lib/localStores";
import type { MarketProposal, MarketProposalAdapter, ProposalChoice } from "./marketProposals";

/**
 * Live watch-only proposals (design-1118a Ventures > Trading) over src-tauri/src/commands/watch_proposals.rs.
 * Synchronous reads over a cache filled on first touch and refreshed after each write. Records his choice
 * only: there is no method here that sends or places anything, and no price or news is invented. The market
 * line and the news note stay unwired until the gated feeds are connected (`market_feed_status` says
 * `not_connected`).
 */
export type WatchDeps = { call: <T>(command: string, args?: Record<string, unknown>) => Promise<T>; notify: () => void };
const defaultDeps: WatchDeps = { call: (c, a) => invokeBound(c, a), notify: announceStores };

const SIDES = new Set(["buy", "sell", "watch"]);
const KINDS = new Set(["crypto", "stock"]);
const CONF = new Set(["low", "medium", "high"]);
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

type Row = { proposal: MarketProposal; status: string; decidedAt: string | null };

export function mapProposal(raw: unknown): Row | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const id = str(r.id), ticker = str(r.ticker), reason = str(r.reasonText), source = str(r.sourceName), risk = str(r.riskText), status = str(r.status);
  if (!id || !ticker || reason === null || source === null || risk === null || !status) return null;
  if (!SIDES.has(String(r.side)) || !KINDS.has(String(r.kind)) || !CONF.has(String(r.confidence)) || r.sample === true) return null;
  const side = r.side as MarketProposal["side"];
  return {
    status, decidedAt: str(r.decidedAt),
    proposal: { id, side, kind: r.kind as MarketProposal["kind"], symbol: ticker, title: `${cap(side)} ${ticker}`, reason, source, confidence: r.confidence as MarketProposal["confidence"], risk },
  };
}

/** One price: `price` is null when none was ever fetched (never 0). status is open, closed or stale. */
export type WatchQuote = { price: number | null; currency: string | null; fetchedAt: string | null; status: "open" | "closed" | "stale"; error: string | null };
export type WatchPickRow = { ticker: string; kind: "crypto" | "stock"; reason: string; source: string; sourceUrl: string | null; pickedAt: string; nextReviewAt: string; quote: WatchQuote };
export type WatchlistState = {
  picks: WatchPickRow[];
  feed: { state: "not_connected" | "blocked" | "connected" | "stale" | "error"; lastAttemptAt: string | null; lastOkAt: string | null; pollMinutes: number };
  research: { running: boolean; lastOutcome: string | null; lastFinishedAt: string | null; lastProposed: number; nextAllowedAt: string | null };
  /** True while "Want new picks?" is showing in Needs you. */
  reviewDue: boolean;
  disclaimer: string;
};
export type ResearchStartState = "started" | "rate_limited" | "blocked" | "already_running";
/** Extras beyond MarketProposalAdapter (0038). Nothing here sends or places anything. */
export type LiveMarket = MarketProposalAdapter & {
  watchlist(): Wired<WatchlistState>;
  /** Re-reads proposals and the watchlist (call after research, or on a timer while a panel is open). */
  refresh(): Promise<void>;
  /** Starts Amara's research (local tier). New picks arrive as proposals. */
  researchNow(): Promise<Wired<{ state: ResearchStartState; retryAfterMinutes: number | null }>>;
  /** Takes a pick off the list. The History id goes to `undoWatch` within 8 s. */
  dropPick(ticker: string): Promise<Wired<{ historyId: string }>>;
  undoWatch(historyId: string): Promise<boolean>;
  /** The last few recorded or dismissed proposals, newest first, from the same read (no extra command). */
  recent(): Wired<DecidedProposal[]>;
};
export type DecidedProposal = { id: string; title: string; choice: "approved" | "dismissed"; at: string };

const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const STATUS = new Set(["open", "closed", "stale"]);

export function mapWatchlist(raw: unknown): WatchlistState | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const feed = r.feed as Record<string, unknown> | undefined, research = r.research as Record<string, unknown> | undefined;
  if (!Array.isArray(r.picks) || !feed || !research) return null;
  const picks: WatchPickRow[] = [];
  for (const v of r.picks as { pick?: unknown; quote?: unknown }[]) {
    const p = v?.pick as Record<string, unknown> | undefined, q = v?.quote as Record<string, unknown> | undefined;
    if (!p || !q || !str(p.ticker) || !KINDS.has(String(p.kind)) || !STATUS.has(String(q.status))) continue;
    picks.push({
      ticker: p.ticker as string, kind: p.kind as WatchPickRow["kind"], reason: str(p.reasonText) ?? "", source: str(p.sourceName) ?? "", sourceUrl: str(p.sourceUrl),
      pickedAt: str(p.pickedAt) ?? "", nextReviewAt: str(p.nextReviewAt) ?? "",
      quote: { price: num(q.price), currency: str(q.currency), fetchedAt: str(q.fetchedAt), status: q.status as WatchQuote["status"], error: str(q.error) },
    });
  }
  return {
    picks,
    feed: { state: feed.state as WatchlistState["feed"]["state"], lastAttemptAt: str(feed.lastAttemptAt), lastOkAt: str(feed.lastOkAt), pollMinutes: num(feed.pollMinutes) ?? 15 },
    research: { running: research.running === true, lastOutcome: str(research.lastOutcome), lastFinishedAt: str(research.lastFinishedAt), lastProposed: num(research.lastProposed) ?? 0, nextAllowedAt: str(research.nextAllowedAt) },
    reviewDue: r.reviewDue === true,
    disclaimer: str(r.disclaimer) ?? "Not financial advice",
  };
}

export function makeLiveMarket(deps: WatchDeps = defaultDeps): LiveMarket {
  let rows: Row[] | null = null;
  let loading = false;
  let wl: WatchlistState | null = null;
  let wlLoading = false;
  const lastWrite = new Map<string, string>(); // proposal id -> history id, for Undo (add_undo("watch", id))

  async function load(): Promise<void> {
    try {
      const l = await deps.call<{ proposals?: unknown[] }>("watch_proposals_list", { includeSample: false });
      rows = (l?.proposals ?? []).flatMap((p) => { const m = mapProposal(p); return m ? [m] : []; });
    } catch { /* stays as it was */ }
  }
  async function loadWatchlist(): Promise<void> {
    try { wl = mapWatchlist(await deps.call("watchlist_get")) ?? wl; } catch { /* stays as it was */ }
  }
  function ensureWatchlist(): void {
    if (wl === null && !wlLoading) { wlLoading = true; void loadWatchlist().finally(() => { wlLoading = false; deps.notify(); }); }
  }
  function ensure(): void {
    if (rows === null && !loading) { loading = true; void load().finally(() => { loading = false; deps.notify(); }); }
  }
  return {
    proposals: () => { ensure(); return rows ? live(rows.filter((r) => r.status === "proposed").map((r) => r.proposal)) : UNWIRED; },
    market: () => UNWIRED,
    newsNote: () => UNWIRED,
    watchlist: () => { ensureWatchlist(); return wl ? live(wl) : UNWIRED; },
    async refresh() { await Promise.all([load(), loadWatchlist()]); deps.notify(); },
    async researchNow() {
      try {
        const r = await deps.call<{ state?: string; retryAfterMinutes?: number | null }>("watchlist_research_now");
        const state = r?.state as ResearchStartState;
        if (!["started", "rate_limited", "blocked", "already_running"].includes(state)) return UNWIRED;
        await loadWatchlist(); deps.notify();
        return live({ state, retryAfterMinutes: num(r.retryAfterMinutes) });
      } catch { return UNWIRED; }
    },
    async dropPick(ticker) {
      try {
        const d = await deps.call<{ id?: string }>("watchlist_drop", { ticker, via: "typed" });
        if (typeof d?.id !== "string") return UNWIRED;
        await loadWatchlist(); deps.notify();
        return live({ historyId: d.id });
      } catch { return UNWIRED; }
    },
    async undoWatch(historyId) {
      let ok = false;
      try { ok = (await deps.call<boolean>("add_undo", { kind: "watch", id: historyId })) === true; } catch { return false; }
      await Promise.all([load(), loadWatchlist()]); deps.notify();
      return ok;
    },
    recent: (): Wired<DecidedProposal[]> => {
      ensure();
      if (!rows) return UNWIRED;
      const done = rows.filter((r) => (r.status === "recorded" || r.status === "dismissed") && r.decidedAt)
        .map((r) => ({ id: r.proposal.id, title: r.proposal.title, choice: r.status === "recorded" ? "approved" as const : "dismissed" as const, at: r.decidedAt as string }));
      return live(done.sort((a, b) => (a.at < b.at ? 1 : a.at > b.at ? -1 : 0)).slice(0, 5));
    },
    choices: (): Wired<ProposalChoice[]> => {
      ensure();
      return rows ? live(rows.filter((r) => r.status === "recorded" && r.decidedAt).map((r) => ({ proposalId: r.proposal.id, choice: "approved" as const, recordedAt: r.decidedAt as string }))) : UNWIRED;
    },
    async recordChoice(proposalId, choice) {
      const command = choice === "approved" ? "watch_proposal_record" : "watch_proposal_dismiss";
      try {
        const d = await deps.call<{ id?: string; toast?: string }>(command, { id: proposalId, via: "typed" });
        if (typeof d?.id === "string") lastWrite.set(proposalId, d.id);
        // An approval adds the pick to the watchlist in the same write, so re-read both.
        await Promise.all([load(), loadWatchlist()]);
        deps.notify();
        return live({ proposalId, choice, recordedAt: new Date().toISOString(), ...(typeof d?.toast === "string" ? { toast: d.toast } : {}), ...(typeof d?.id === "string" ? { historyId: d.id } : {}) });
      } catch { return UNWIRED; }
    },
  };
}
