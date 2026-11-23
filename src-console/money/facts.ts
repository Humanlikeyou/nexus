import { BANNED_PHRASES } from "../amaraTake/bannedPhrases";
import { matches, numbersIn } from "../amaraTake/validate";
import { CAUSAL } from "../takes/causal";
import { MONEY_BANNED, NFA_LINE } from "./adapters";

/**
 * Logic only: maps `money_facts_read` and gates a money take on it. There is NO money take copy in the catalog
 * yet (design-questions 2026-11-14), so nothing here writes or generates text; `checkMoneyText` is the validator
 * a future take must pass. A take may cite only numbers that a fact with a source ref carries.
 */

export type MoneyFactWire = { key: string; label: string; value: number; unit: string; as_of: string | null; source: string; stale: boolean };
export type MoneyFactsWire = { day: string; facts: MoneyFactWire[]; partial: boolean; partial_reasons: string[]; personal_accounts: number };
export type MoneyFact = { key: string; label: string; value: number; unit: string; asOf: string | null; source: string; stale: boolean };
export type MoneyFacts = { day: string; facts: MoneyFact[]; partial: boolean; reasons: string[]; personalAccounts: number };

/** A fact with no source ref, no finite value or no key is dropped, never cited. Non-array or missing input is null. */
export function mapFacts(raw: MoneyFactsWire | null | undefined): MoneyFacts | null {
  if (!raw || !Array.isArray(raw.facts) || typeof raw.day !== "string") return null;
  const facts = raw.facts
    .filter((f) => f && typeof f.key === "string" && f.key && typeof f.source === "string" && f.source && typeof f.value === "number" && Number.isFinite(f.value))
    .map((f) => ({ key: f.key, label: f.label, value: f.value, unit: f.unit, asOf: f.as_of ?? null, source: f.source, stale: !!f.stale }));
  return { day: raw.day, facts, partial: !!raw.partial || !Array.isArray(raw.partial_reasons) || facts.length !== raw.facts.length, reasons: Array.isArray(raw.partial_reasons) ? raw.partial_reasons.filter((r) => typeof r === "string") : [], personalAccounts: Number(raw.personal_accounts) || 0 };
}

export type TakeGate = { mode: "none" } | { mode: "partial"; reasons: string[] } | { mode: "ok" };

/** none: nothing to say. partial: any personal account stale or unknown, so no suggestion (note only). ok: every account fresh. */
export function moneyTakeGate(f: MoneyFacts | null): TakeGate {
  if (!f || f.personalAccounts === 0 || f.facts.length === 0) return { mode: "none" };
  if (f.partial || f.facts.some((x) => x.stale && x.unit !== "JPY per USD")) return { mode: "partial", reasons: f.reasons };
  return { mode: "ok" };
}

export type MoneyTextCheck = { ok: true } | { ok: false; reason: "empty" | "currency_sign" | "banned" | "causal" | "no_nfa" | "no_source" | "unsupported_number" | "suggestion_while_partial" };

/**
 * The validator a money take must pass. `suggestion` marks text that proposes something to do; it is refused under a
 * partial gate. Text must end with the fixed NFA line and may use only numbers that a sourced fact carries
 * (the NFA line itself has none). Rejected text is never patched.
 */
export function checkMoneyText(raw: string, facts: MoneyFacts | null, suggestion = false): MoneyTextCheck {
  const text = raw.trim();
  if (!text) return { ok: false, reason: "empty" };
  if (/[¥￥]/.test(text)) return { ok: false, reason: "currency_sign" };
  if (MONEY_BANNED.test(text) || BANNED_PHRASES.some((r) => r.test(text))) return { ok: false, reason: "banned" };
  if (CAUSAL.test(text)) return { ok: false, reason: "causal" };
  if (!text.endsWith(NFA_LINE)) return { ok: false, reason: "no_nfa" };
  const gate = moneyTakeGate(facts);
  if (suggestion && gate.mode !== "ok") return { ok: false, reason: "suggestion_while_partial" };
  const body = text.slice(0, text.length - NFA_LINE.length);
  const nums = numbersIn(body);
  if (nums.length && (!facts || facts.facts.length === 0)) return { ok: false, reason: "no_source" };
  const allowed = (facts?.facts ?? []).map((f) => f.value);
  if (nums.some((n) => !allowed.some((a) => matches(n, a)))) return { ok: false, reason: "unsupported_number" };
  return { ok: true };
}
