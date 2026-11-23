import { CATALOG, type CatalogEntry } from "../amaraTake/catalog";
import { validateTake } from "../amaraTake/validate";
import type { Fact, FactSet, TakePage } from "../amaraTake/types";
import { daysBetween, daysSinceKind, readinessLevel, seeded, seriesLevel, sportKind, type OfferRecord, type PickCtx } from "./variety";
import type { TakeKind } from "../amaraTake/catalog";
import { CAUSAL } from "./causal";
import type { Fact as ViewFact } from "./adapters";

/** Deterministic selection (HANDOFF-1111t §3). Pure: the same facts, catalog and history always pick the same entry. */
export const MIN_BASELINE = 7;
const LEVEL_RANK: Record<string, number> = { strong: 4, moderate: 3, early: 2, own: 1 };

const byKey = (set: FactSet, key: string): Fact | null => set.facts.find((f) => f.key === key) ?? null;

/** Gate (§3.1): nothing is suggested until some fact has its 7+ day baseline. Business is own data and has no baseline gate. */
export function gate(set: FactSet): { ok: boolean; have: number } {
  if (set.page === "business") return { ok: set.facts.length > 0, have: set.facts.length };
  const have = Math.max(0, ...set.facts.map((f) => f.priorN ?? (f.baseline?.n ?? 0)));
  return { ok: set.facts.some((f) => f.baseline !== null && f.baseline.n >= MIN_BASELINE), have };
}

type Cnd = { factKey: string; op: "below_baseline" | "above_baseline" | "below" | "above"; by?: number };
type Hit = { met: boolean; distance: number; fact: Fact | null };

/** Value of a fact key, or a derived one: readiness, hour, loadRatio, daysSince:<kind>. Undefined means unknown (a condition on it is unmet; a contraindication on it does not block). */
function derived(key: string, set: FactSet, ctx: PickCtx): number | undefined {
  if (key === "readiness") return readinessLevel(set);
  if (key === "hour") return ctx.hour;
  if (key === "loadRatio") return ctx.loadRatio ?? undefined;
  if (key.startsWith("daysSince:")) return daysSinceKind(ctx.workouts, key.slice(10), set.day);
  return undefined;
}

function evalCond(c: Cnd, set: FactSet, ctx: PickCtx): Hit {
  const f = byKey(set, c.factKey);
  const by = c.by ?? 0;
  if (c.op === "below_baseline" || c.op === "above_baseline") {
    if (!f || !f.baseline || f.baseline.n < MIN_BASELINE || f.baseline.mean === 0) return { met: false, distance: 0, fact: f };
    const rel = (f.value - f.baseline.mean) / Math.abs(f.baseline.mean);
    return { met: c.op === "below_baseline" ? rel < -by : rel > by, distance: Math.abs(rel), fact: f };
  }
  const v = f ? f.value : derived(c.factKey, set, ctx);
  if (v === undefined) return { met: false, distance: 0, fact: f };
  return { met: c.op === "below" ? v < by : v > by, distance: Math.abs(v - by) / Math.max(1, Math.abs(by)), fact: f };
}

/** Does the entry's trigger fire on these facts, and how far from usual is it (0..1+, for ranking)? Old `trigger` and new `triggers` {all, any} both work; no trigger at all always fires. */
export function evalTrigger(entry: CatalogEntry, set: FactSet, ctx: PickCtx = {}): { fires: boolean; distance: number; fact: Fact | null } {
  const all = entry.triggers?.all ?? [], any = entry.triggers?.any ?? [];
  const conds: Cnd[] = entry.triggers ? all : entry.trigger ? [entry.trigger] : [];
  const hits = conds.map((c) => evalCond(c, set, ctx));
  const anyHits = any.map((c) => evalCond(c, set, ctx));
  const fires = hits.every((h) => h.met) && (!any.length || anyHits.some((h) => h.met));
  const used = [...hits, ...anyHits.filter((h) => h.met)];
  const lead = used.find((h) => h.fact && h.met) ?? used.find((h) => h.fact) ?? null;
  return { fires, distance: used.reduce((m, h) => Math.max(m, h.distance), 0), fact: lead?.fact ?? set.facts[0] ?? null };
}

export const blocked = (entry: CatalogEntry, set: FactSet, ctx: PickCtx = {}) => (entry.contraindications ?? []).some((c) => evalCond({ factKey: c.factKey, op: c.op, by: c.value ?? c.by }, set, ctx).met);

export type PickInput = { page: TakePage; set: FactSet; entries?: CatalogEntry[]; stopped?: ReadonlySet<string>; cooldownUntil?: ReadonlyMap<string, string>; day: string; ctx?: PickCtx };
export type Pick = { entry: CatalogEntry | null; fact: Fact | null; alsoConsidered: CatalogEntry[] };

/** How well the session's intensity matches today's readiness (0..1). Unknown readiness favours easy and moderate. */
function fit(e: CatalogEntry, readiness: number | undefined): number {
  const i = e.intensity ?? "moderate";
  if (readiness === undefined) return i === "easy" ? 0.6 : i === "moderate" ? 0.5 : 0;
  if (readiness === 3) return i === "hard" ? 1 : i === "moderate" ? 0.7 : 0.35;
  if (readiness === 2) return i === "moderate" ? 1 : i === "easy" ? 0.6 : 0.2;
  return i === "easy" ? 1 : 0;
}

/** Score = fit (readiness) + evidence + distance from usual, minus a recent-offer penalty and a same-kind penalty, plus a small seeded-by-day jitter. */
export function scoreEntry(e: CatalogEntry, distance: number, inp: PickInput, offers: readonly OfferRecord[], lastKinds: { offered: TakeKind | undefined; offered2: TakeKind | undefined; did: TakeKind | null }): number {
  const base = fit(e, readinessLevel(inp.set)) * 2 + (LEVEL_RANK[e.evidence.level] ?? 0) / 4 + Math.min(distance, 1) * 0.5;
  const window = Math.min(e.cooldownDays, 14);
  const ago = offers.filter((o) => o.recId === e.id).map((o) => daysBetween(o.day, inp.day)).filter((d) => d > 0 && d < window);
  const recency = ago.length ? 3 * (1 - Math.min(...ago) / window) + 0.3 * (ago.length - 1) : 0;
  const kind = e.kind;
  const kindPen = kind ? (kind === lastKinds.offered ? 1.5 : 0) + (kind === lastKinds.did ? 1.5 : 0) + (kind === lastKinds.offered2 ? 0.5 : 0) : 0;
  return base - recency - kindPen + seeded(inp.day, e.id) * 0.5;
}

/**
 * Selection: filter by trigger, contraindication, stop and cooldown, keep only the current level of a progressive series,
 * drop anything offered in the last few days when an alternative exists, then rank by score.
 * Weak entries are never picked. The same day, facts and history always give the same entry; another day differs.
 */
export function pickEntry(inp: PickInput): Pick {
  const ctx = inp.ctx ?? {}, offers = (ctx.offers ?? []).filter((o) => o.day < inp.day);
  const all = inp.entries ?? CATALOG.entries;
  const entries = all.filter((e) => e.page === inp.page);
  const scored = entries.map((e) => ({ e, t: evalTrigger(e, inp.set, ctx) })).filter((x) => x.t.fires && !blocked(x.e, inp.set, ctx));
  const eligible = scored.filter((x) => x.e.evidence.level !== "weak" && !inp.stopped?.has(x.e.id) && !((inp.cooldownUntil?.get(x.e.id) ?? "") > inp.day)
    && (!x.e.progression || x.e.progression.level === seriesLevel(x.e.progression.series, all, offers)));
  const recent = (id: string, within: number) => offers.some((o) => o.recId === id && daysBetween(o.day, inp.day) < within);
  const fresh = eligible.filter((x) => !recent(x.e.id, Math.min(x.e.cooldownDays, 6)));
  const pool = fresh.length ? fresh : eligible;
  // Kinds of the last offer, of the offer before it, and of what he actually trained yesterday.
  const kindOn = (d: string) => { const o = offers.find((x) => x.day === d); return o ? entries.find((e) => e.id === o.recId)?.kind : undefined; };
  const yesterday = addDay(inp.day, -1);
  const didSport = (ctx.workouts ?? []).filter((w) => w.day === yesterday).map((w) => sportKind(w.sport)).find((k) => k !== null) ?? null;
  const lastKinds = { offered: kindOn(yesterday), offered2: kindOn(addDay(inp.day, -2)), did: didSport };
  const ranked = pool.map((x) => ({ ...x, score: scoreEntry(x.e, x.t.distance, inp, offers, lastKinds) }))
    .sort((a, b) => b.score - a.score || (LEVEL_RANK[b.e.evidence.level] ?? 0) - (LEVEL_RANK[a.e.evidence.level] ?? 0) || b.t.distance - a.t.distance || a.e.id.localeCompare(b.e.id));
  const top = ranked[0];
  const weak = scored.filter((x) => x.e.evidence.level === "weak").map((x) => x.e);
  const runners = ranked.slice(1, 3).map((x) => x.e);
  const also = [...new Set([...weak, ...(entries.length > 6 ? runners : scored.map((x) => x.e))])].filter((e) => e !== top?.e).sort((a, b) => a.id.localeCompare(b.id));
  return { entry: top?.e ?? null, fact: top?.t.fact ?? null, alsoConsidered: also };
}

const addDay = (day: string, n: number) => { const t = Date.parse(`${day}T00:00:00Z`) + n * 86_400_000; return Number.isFinite(t) ? new Date(t).toISOString().slice(0, 10) : ""; };

// ---------------- the why sentence ----------------
export function buildWhyPrompt(entry: CatalogEntry, facts: Fact[]): string {
  return [
    `Taisun's page suggests today: "${entry.title}"`,
    "Explain in one sentence why this suggestion fits the facts below. Quote at least one number exactly as shown. Use only these facts. Do not invent numbers, causes or promises. Do not say it causes, proves or fixes anything. No currency symbols. Plain text, no markdown.",
    "Facts:",
    ...facts.map((f) => `- ${f.label}: ${f.display}${f.baseline ? ` (usual over the last ${f.baseline.n} days: ${f.baseline.display})` : ""}`),
  ].join("\n");
}

/** Template why built from the trigger fact only. Never patched model text. Business states its own number only. */
export function templateWhy(fact: Fact): string {
  if (!fact.baseline) return `${fact.label} is **${fact.display}**.`;
  const dir = fact.value < fact.baseline.mean ? "below" : fact.value > fact.baseline.mean ? "above" : "in line with";
  return `${fact.label} is **${fact.display}**, ${dir} your usual **${fact.baseline.display}**.`;
}

/** Accepts a model sentence only if it passes validateTake (cites a fact, no invented numbers) and the CAUSAL guard. */
export function acceptWhy(raw: string, facts: Fact[]): string | null {
  const v = validateTake(raw, facts);
  if (!v.ok || CAUSAL.test(v.text) || v.text.length > 300) return null;
  return v.text;
}

export async function whyFor(entry: CatalogEntry, fact: Fact, set: FactSet, ask: (prompt: string) => Promise<{ content: string; tier: string }>): Promise<{ text: string; source: "model" | "template" }> {
  // Business is "own data only": the sentence may use the trigger fact and nothing else.
  const used = entry.page === "business" ? [fact] : [fact, ...set.facts.filter((f) => f.key !== fact.key).slice(0, 2)];
  try {
    const reply = await ask(buildWhyPrompt(entry, used));
    if (reply.tier !== "claude") { const ok = acceptWhy(reply.content, used); if (ok) return { text: ok, source: "model" }; }
  } catch { /* falls to the template */ }
  return { text: templateWhy(fact), source: "template" };
}

/** The numbers shown under the why: the trigger fact first, then up to two more. */
export function viewFacts(fact: Fact, set: FactSet): ViewFact[] {
  const note = (f: Fact) => (f.baseline ? `usual ${f.baseline.display}` : "today");
  return [fact, ...set.facts.filter((f) => f.key !== fact.key).slice(0, 2)].map((f) => ({ label: f.label, value: f.display, note: note(f) }));
}
