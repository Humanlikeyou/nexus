import type { ContentDraft, ContentStageSet, FinancesView_Serialize, VentureTracking_Serialize } from "../bindings.gen";
import { invokeBound } from "../bridge";
import { UNWIRED, type Wired } from "../lib/sampleAdapters";
import { announceStores, inTauri, live } from "../lib/localStores";

/**
 * design-1118a lane B4: live Finances, venture tracking and Content (Draft with Amara, stage moves) over
 * src-tauri/src/commands/{enterprise_views,content_pipeline}.rs. Logic only: no markup, class or token.
 *
 * Reads are synchronous over a cache filled on first touch and refreshed after each write (same shape as
 * `marketProposalsLive`). Rules:
 * - Missing is `unwired` ("Needs wiring"), never 0 and never a made-up row. No ledger, or a ledger that
 *   will not load, is `unwired`.
 * - Dollars come only from the business ledger's dated rate. `fx()` carries its `asOf` and `stale` flag;
 *   `usd` fields are null without a rate, so Yen stays NEEDS_CONVERSION until the ledger has one.
 *   Raw yen is data only: nothing here formats a yen sign.
 * - Business only. Personal money (Life › Personal money) is a different store and is never read here.
 * - No method here posts or sends anything. A stage move to Published records that Taisun posted it.
 */
export type Finances = FinancesView_Serialize;
export type Tracking = VentureTracking_Serialize;
export type EnterpriseDeps = { call: <T>(command: string, args?: Record<string, unknown>) => Promise<T>; notify: () => void };
const defaultDeps: EnterpriseDeps = { call: (c, a) => invokeBound(c, a), notify: announceStores };

export type ContentStage = "ideation" | "drafting" | "review" | "scheduled" | "published";

export interface EnterpriseLive {
  /** The whole view once the ledger loaded; `unwired` for no ledger / invalid ledger / not yet read. */
  finances(): Wired<Finances>;
  /** The ledger's dated rate with `asOf` and `stale`; `unwired` when there is no rate. */
  fx(): Wired<NonNullable<Finances["fx"]>>;
  /** Fund rows; `unwired` until the ledger has funds (the page hides rows until there is a balance). */
  funds(): Wired<Finances["funds"]>;
  /** One status row per Treasury source; `unwired` when there are none. */
  sources(): Wired<Finances["sources"]>;
  /** Ledger ventures with Treasury income, attention and per-hour; `unwired` without a ledger. */
  ventures(): Wired<Tracking["ventures"]>;
  /** Draft text from the local tier. Writes nothing. Failure is `unwired`. */
  draft(title: string): Promise<Wired<ContentDraft>>;
  /** One move through the single write path (History + 8 s Undo). `postedByMe` only for Published. */
  setStage(title: string, from: ContentStage, to: ContentStage, postedByMe?: boolean): Promise<Wired<ContentStageSet>>;
  /** Undo within 8 s via the shared write path. */
  undoStage(historyId: string): Promise<boolean>;
}

const isObj = (x: unknown): x is Record<string, unknown> => !!x && typeof x === "object" && !Array.isArray(x);
const okFinances = (x: unknown): x is Finances => isObj(x) && typeof x.ledger === "string" && Array.isArray(x.funds) && Array.isArray(x.sources);
const okTracking = (x: unknown): x is Tracking => isObj(x) && typeof x.ledger === "string" && Array.isArray(x.ventures);

export function makeEnterpriseLive(deps: EnterpriseDeps = defaultDeps): EnterpriseLive {
  let fin: Finances | null = null;
  let trk: Tracking | null = null;
  let loading = false;

  async function load(): Promise<void> {
    const [f, t] = await Promise.all([
      deps.call<unknown>("finances_view").catch(() => null),
      deps.call<unknown>("venture_tracking_list").catch(() => null),
    ]);
    if (okFinances(f)) fin = f;
    if (okTracking(t)) trk = t;
  }
  function ensure(): void {
    if (fin === null && trk === null && !loading) { loading = true; void load().finally(() => { loading = false; deps.notify(); }); }
  }
  const ledgerOk = <T extends { ledger: string }>(v: T | null): v is T => v !== null && v.ledger === "ok";

  return {
    finances: () => { ensure(); return ledgerOk(fin) ? live(fin) : UNWIRED; },
    fx: () => { ensure(); return ledgerOk(fin) && fin.fx && fin.fx.usdPerJpy !== null ? live(fin.fx) : UNWIRED; },
    funds: () => { ensure(); return ledgerOk(fin) && fin.funds.length > 0 ? live(fin.funds) : UNWIRED; },
    sources: () => { ensure(); return ledgerOk(fin) && fin.sources.length > 0 ? live(fin.sources) : UNWIRED; },
    ventures: () => { ensure(); return ledgerOk(trk) && trk.ventures.length > 0 ? live(trk.ventures) : UNWIRED; },
    async draft(title) {
      try { return live(await deps.call<ContentDraft>("content_draft_propose", { title })); } catch { return UNWIRED; }
    },
    async setStage(title, from, to, postedByMe = false) {
      try {
        const r = await deps.call<ContentStageSet>("content_stage_set", { title, from, to, postedByMe: to === "published" && postedByMe, via: "typed" });
        deps.notify();
        return live(r);
      } catch { return UNWIRED; }
    },
    async undoStage(historyId) {
      try { const ok = await deps.call<boolean>("add_undo", { kind: "content", id: historyId }); deps.notify(); return ok === true; } catch { return false; }
    },
  };
}

/** Outside Tauri there is nothing to read: every read is unwired. */
export const unwiredEnterprise: EnterpriseLive = {
  finances: () => UNWIRED, fx: () => UNWIRED, funds: () => UNWIRED, sources: () => UNWIRED, ventures: () => UNWIRED,
  draft: async () => UNWIRED, setStage: async () => UNWIRED, undoStage: async () => false,
};

let shared: EnterpriseLive | null = null;
/** The live adapter inside Tauri (one shared cache), the unwired one in a browser or test. */
export const enterpriseAdapter = (): EnterpriseLive => (inTauri() ? (shared ??= makeEnterpriseLive()) : unwiredEnterprise);
