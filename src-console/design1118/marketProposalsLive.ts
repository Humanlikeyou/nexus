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

export function makeLiveMarket(deps: WatchDeps = defaultDeps): MarketProposalAdapter {
  let rows: Row[] | null = null;
  let loading = false;
  const lastWrite = new Map<string, string>(); // proposal id -> history id, for Undo (add_undo("watch", id))

  async function load(): Promise<void> {
    try {
      const l = await deps.call<{ proposals?: unknown[] }>("watch_proposals_list", { includeSample: false });
      rows = (l?.proposals ?? []).flatMap((p) => { const m = mapProposal(p); return m ? [m] : []; });
    } catch { /* stays as it was */ }
  }
  function ensure(): void {
    if (rows === null && !loading) { loading = true; void load().finally(() => { loading = false; deps.notify(); }); }
  }
  return {
    proposals: () => { ensure(); return rows ? live(rows.filter((r) => r.status === "proposed").map((r) => r.proposal)) : UNWIRED; },
    market: () => UNWIRED,
    newsNote: () => UNWIRED,
    choices: (): Wired<ProposalChoice[]> => {
      ensure();
      return rows ? live(rows.filter((r) => r.status === "recorded" && r.decidedAt).map((r) => ({ proposalId: r.proposal.id, choice: "approved" as const, recordedAt: r.decidedAt as string }))) : UNWIRED;
    },
    async recordChoice(proposalId, choice) {
      const command = choice === "approved" ? "watch_proposal_record" : "watch_proposal_dismiss";
      try {
        const d = await deps.call<{ id?: string }>(command, { id: proposalId, via: "typed" });
        if (typeof d?.id === "string") lastWrite.set(proposalId, d.id);
      } catch { return UNWIRED; }
      await load();
      deps.notify();
      return live({ proposalId, choice, recordedAt: new Date().toISOString() });
    },
  };
}
