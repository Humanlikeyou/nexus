import { factLines } from "./facts";
import { loadStored, MAX_ATTEMPTS_PER_DAY, saveStored, type KvDeps } from "./store";
import { validateTake } from "./validate";
import { tokyoDay } from "../lib/mindBody";
import { UNWIRED } from "../lib/sampleAdapters";
import type { AmaraTake, FactSet, TakePage, TakeResult } from "./types";

const PAGE_ABOUT: Record<TakePage, string> = {
  fitness: "Taisun's fitness page", neuro: "Taisun's Muse neurofeedback page", mindBody: "Taisun's mind-body page",
  business: "Taisun's business page", home: "Taisun's home page", life: "Taisun's Life pages (goals, weekly review, dates and personal money)",
};

/** The facts are the only data in the prompt. The router adds SOUL.md and the identity guard. */
export function buildPrompt(set: FactSet): string {
  return [
    `Write Amara's one short comment (1 to 3 sentences, plain text, no markdown) for ${PAGE_ABOUT[set.page]} on ${set.day}.`,
    "Use only the facts below. Quote at least one number exactly as shown. Say whether it is above, below or in line with the usual value when one is given. Do not invent numbers, causes or advice beyond them. Do not add currency symbols or name a currency.",
    "Facts:",
    factLines(set),
  ].join("\n");
}

export type TakeDeps = {
  now: () => number;
  kv: KvDeps;
  /** Light-tier router call; resolves to the reply text and the tier that answered. */
  ask: (prompt: string) => Promise<{ content: string; tier: string }>;
  /** Today's facts for the page, or null when that day's data does not exist yet. */
  facts: (page: TakePage, today: string) => Promise<FactSet | null>;
};

/**
 * One pass for a page: cache hit, miss, or refresh. At most MAX_ATTEMPTS_PER_DAY router calls per page per day
 * (first take plus one refresh when the fact fingerprint changes). A failed or rejected reply stays unwired,
 * except that an earlier valid take from the same day is kept when a refresh fails.
 */
export async function runTake(page: TakePage, deps: TakeDeps): Promise<TakeResult> {
  const today = tokyoDay(new Date(deps.now()).toISOString()) ?? "";
  let set: FactSet | null = null;
  try { set = await deps.facts(page, today); } catch { set = null; }
  if (!set || !set.facts.length) return UNWIRED;
  const stored = await loadStored(page, today, deps.kv);
  const keep = (): TakeResult => (stored.take ? { state: "live", value: stored.take } : UNWIRED);
  if (stored.take && stored.fingerprint === set.fingerprint) return keep();
  if (stored.attempts >= MAX_ATTEMPTS_PER_DAY) return keep();
  const attempt = { ...stored, attempts: stored.attempts + 1 };
  let take: AmaraTake | null = null;
  try {
    const reply = await deps.ask(buildPrompt(set));
    // A cloud answer would be a path the gate did not approve for this feature: not used.
    const verdict = reply.tier === "claude" ? null : validateTake(reply.content, set.facts);
    if (verdict?.ok) take = { page, day: today, text: verdict.text, cited: verdict.cited, fingerprint: set.fingerprint, generatedAt: deps.now() };
  } catch { take = null; }
  // Record the attempt either way so a bad reply cannot loop; the fingerprint is kept only with a take.
  await saveStored(page, take ? { day: today, attempts: attempt.attempts, fingerprint: set.fingerprint, take } : attempt, deps.kv);
  if (take) return { state: "live", value: take };
  return keep();
}
