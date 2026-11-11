import { CATALOG, type CatalogEntry } from "../amaraTake/catalog";
import { validateTake } from "../amaraTake/validate";
import type { Fact, FactSet, TakePage } from "../amaraTake/types";
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

/** Does the entry's trigger fire on these facts, and how far from usual is it (0..1+, for ranking)? */
export function evalTrigger(entry: CatalogEntry, set: FactSet): { fires: boolean; distance: number; fact: Fact | null } {
  const f = byKey(set, entry.trigger.factKey);
  if (!f) return { fires: false, distance: 0, fact: null };
  const by = entry.trigger.by ?? 0;
  switch (entry.trigger.op) {
    case "below_baseline": case "above_baseline": {
      if (!f.baseline || f.baseline.n < MIN_BASELINE || f.baseline.mean === 0) return { fires: false, distance: 0, fact: f };
      const rel = (f.value - f.baseline.mean) / Math.abs(f.baseline.mean);
      const fires = entry.trigger.op === "below_baseline" ? rel < -by : rel > by;
      return { fires, distance: Math.abs(rel), fact: f };
    }
    case "below": return { fires: f.value < by, distance: Math.abs(f.value - by) / Math.max(1, Math.abs(by)), fact: f };
    case "above": return { fires: f.value > by, distance: Math.abs(f.value - by) / Math.max(1, Math.abs(by)), fact: f };
  }
}

const blocked = (entry: CatalogEntry, set: FactSet) => (entry.contraindications ?? []).some((c) => {
  const f = byKey(set, c.factKey);
  return !!f && (c.op === "below" ? f.value < c.value : f.value > c.value);
});

export type PickInput = { page: TakePage; set: FactSet; entries?: CatalogEntry[]; stopped?: ReadonlySet<string>; cooldownUntil?: ReadonlyMap<string, string>; day: string };
export type Pick = { entry: CatalogEntry | null; fact: Fact | null; alsoConsidered: CatalogEntry[] };

/** Rank by evidence level (strong > moderate > early > own; weak is never picked), then distance from usual, then id. */
export function pickEntry(inp: PickInput): Pick {
  const entries = (inp.entries ?? CATALOG.entries).filter((e) => e.page === inp.page);
  const scored = entries.map((e) => ({ e, t: evalTrigger(e, inp.set) })).filter((x) => x.t.fires && !blocked(x.e, inp.set));
  const eligible = scored.filter((x) => x.e.evidence.level !== "weak" && !inp.stopped?.has(x.e.id) && !((inp.cooldownUntil?.get(x.e.id) ?? "") > inp.day));
  eligible.sort((a, b) => (LEVEL_RANK[b.e.evidence.level] ?? 0) - (LEVEL_RANK[a.e.evidence.level] ?? 0) || b.t.distance - a.t.distance || a.e.id.localeCompare(b.e.id));
  const top = eligible[0];
  const also = scored.filter((x) => x !== top).map((x) => x.e).sort((a, b) => a.id.localeCompare(b.id));
  return { entry: top?.e ?? null, fact: top?.t.fact ?? null, alsoConsidered: also };
}

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
