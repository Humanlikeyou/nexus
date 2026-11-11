import type { CitedPoint, Fact } from "./types";

export const MAX_TAKE_CHARS = 600;

/** Numbers in a reply, tolerant of "1,234", "7.2" and "1 234"-free forms. */
export function numbersIn(text: string): number[] {
  const out: number[] = [];
  for (const m of text.matchAll(/\d{1,3}(?:,\d{3})+(?:\.\d+)?|\d+(?:\.\d+)?/g)) out.push(Number(m[0].replace(/,/g, "")));
  return out;
}

/** A reply number matches when it equals the fact number, or its rounding to a whole number or one decimal. */
export function matches(n: number, a: number): boolean {
  return Math.abs(n - a) < 0.0001 || Math.abs(n - a) <= 0.05 || (Number.isInteger(n) && Math.round(a) === n) || Number(a.toFixed(1)) === n;
}

export const citedFacts = (text: string, facts: Fact[]): Fact[] => {
  const nums = numbersIn(text);
  return facts.filter((f) => f.accept.some((a) => nums.some((n) => matches(n, a))));
};

export const toCited = (f: Fact): CitedPoint => ({ key: f.key, label: f.label, display: f.display, baseline: f.baseline?.display ?? null });

export type Validation = { ok: true; text: string; cited: CitedPoint[] } | { ok: false; reason: "empty" | "too_long" | "currency" | "no_fact_cited" | "unsupported_number" };

/** Every number a reply may contain: each fact's accepted values, its baseline mean and the baseline window size. */
export function allowedNumbers(facts: Fact[]): number[] {
  return facts.flatMap((f) => [...f.accept, ...(f.baseline ? [f.baseline.mean, f.baseline.n] : [])]);
}

/** Accepts a reply only when it cites at least one fact value and contains no number outside the facts (no invented figures). Rejected text is never patched. */
export function validateTake(raw: string, facts: Fact[]): Validation {
  const text = raw.trim();
  if (!text) return { ok: false, reason: "empty" };
  if (text.length > MAX_TAKE_CHARS) return { ok: false, reason: "too_long" };
  if (/[¥$€£]|\b(?:yen|jpy)\b/i.test(text)) return { ok: false, reason: "currency" };
  const allowed = allowedNumbers(facts);
  if (numbersIn(text).some((n) => !allowed.some((a) => matches(n, a)))) return { ok: false, reason: "unsupported_number" };
  const cited = citedFacts(text, facts);
  return cited.length ? { ok: true, text, cited: cited.map(toCited) } : { ok: false, reason: "no_fact_cited" };
}
