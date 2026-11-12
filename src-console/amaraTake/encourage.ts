import { CAUSAL } from "../takes/causal";
import { BANNED_PHRASES } from "./bannedPhrases";
import { allowedNumbers, matches, numbersIn } from "./validate";
import type { Fact, TakePage } from "./types";

/** A short, grounded lift for a take: one or two sentences about mind, body, spirit or leadership. Pure; local data only. */
export type Dimension = "mind" | "body" | "spirit" | "leadership";
export const DIMENSIONS: Dimension[] = ["mind", "body", "spirit", "leadership"];
export const NO_REPEAT_DAYS = 14;
const DAY_MS = 86_400_000;

/** Advice that reads as medical. The lift never gives any. */
const MEDICAL = /\b(?:diagnos\w*|treat(?:ment)?|medicat\w*|doctor|symptoms?|disorder|therapy|prescri\w*)\b/i;

export const hasBannedPhrase = (text: string) => BANNED_PHRASES.some((r) => r.test(text));
export const sentenceCount = (text: string) => (text.trim().match(/[^.!?]+[.!?]+(?=\s|$)|[^.!?]+$/g) ?? []).filter((s) => s.trim()).length;

export type LiftCheck = { ok: true } | { ok: false; reason: "empty" | "too_many_sentences" | "banned_phrase" | "causal" | "medical" | "unsupported_number" };

/** The same guards as a take: at most two sentences, no cliché, no causal or medical wording, and every number traceable to the facts. */
export function validateLift(text: string, facts: Fact[]): LiftCheck {
  const t = text.trim();
  if (!t) return { ok: false, reason: "empty" };
  if (sentenceCount(t) > 2) return { ok: false, reason: "too_many_sentences" };
  if (hasBannedPhrase(t)) return { ok: false, reason: "banned_phrase" };
  if (CAUSAL.test(t)) return { ok: false, reason: "causal" };
  if (MEDICAL.test(t)) return { ok: false, reason: "medical" };
  const allowed = allowedNumbers(facts);
  if (numbersIn(t).some((n) => !allowed.some((a) => matches(n, a)))) return { ok: false, reason: "unsupported_number" };
  return { ok: true };
}

/** Which dimensions fit a page, in preference order. Goals (a goal fact) lean on leadership and spirit. */
const PAGE_DIMS: Record<TakePage, Dimension[]> = {
  fitness: ["body"], mindBody: ["body", "mind", "spirit"], neuro: ["mind", "spirit"],
  home: ["spirit", "body"], business: ["leadership"], life: ["leadership", "spirit", "mind"],
};

type Template = { dim: Dimension; text: string; /** Needs a fact: `{fact}` becomes its label and display, both from the facts. */ fact?: boolean; goal?: boolean };

export const TEMPLATES: Template[] = [
  { dim: "body", text: "Your body is doing steady work today, and listening to it is a strength." },
  { dim: "body", text: "Showing up for your body, one day at a time, is how it stays on your side." },
  { dim: "body", fact: true, text: "{fact} is a real reading of the care you keep putting in." },
  { dim: "mind", text: "A clear, patient mind is built session by session, and you keep turning up." },
  { dim: "mind", text: "Choosing to pay attention to your own mind is quiet, real work." },
  { dim: "mind", fact: true, text: "{fact} shows you are paying attention, and that matters." },
  { dim: "spirit", text: "There is steadiness in how you keep your practice, and it is worth honouring." },
  { dim: "spirit", text: "Take a breath here; the way you keep going says a lot about what you value." },
  { dim: "leadership", text: "Leading well starts with the calm you bring to your own day, and you bring it." },
  { dim: "leadership", text: "Steady follow-through is what people learn to trust in a leader." },
  { dim: "leadership", fact: true, text: "{fact} is the kind of honest tracking good leaders rely on." },
  { dim: "leadership", goal: true, fact: true, text: "{fact}: that is real movement toward a goal you chose, and it counts." },
  { dim: "spirit", goal: true, fact: true, text: "{fact} is a goal moving forward because you keep coming back to it." },
];

type LogLike = { text: string; generatedAt: number };

/** Template lines found in the recent take log, with the dimension each belongs to. Prefix before `{fact}` is matched. */
function usage(log: readonly LogLike[], now: number) {
  const recent = log.filter((e) => now - e.generatedAt < NO_REPEAT_DAYS * DAY_MS);
  const used = new Set<number>();
  const perDim: Record<Dimension, number> = { mind: 0, body: 0, spirit: 0, leadership: 0 };
  TEMPLATES.forEach((t, i) => {
    const tail = t.text.split("{fact}").pop()!.trim();
    if (recent.some((e) => (tail ? e.text.includes(tail) : false) || e.text.includes(t.text))) { used.add(i); perDim[t.dim] += 1; }
  });
  return { used, perDim, recent };
}

const factText = (f: Fact) => `${f.label} at ${f.display}`;

export type LiftContext = { page: TakePage; facts: Fact[]; log: readonly LogLike[]; now: number };

/**
 * The template lift: the least-engaged fitting dimension first (counted from lines in the last 14 days of the
 * log), skipping any line already used in that window. Null when every fitting line was used. After three or
 * more days of takes in the last week, a plain follow-through sentence opens the lift (it carries no number).
 */
export function templateLift(ctx: LiftContext): string | null {
  const { used, perDim, recent } = usage(ctx.log, ctx.now);
  const dims = PAGE_DIMS[ctx.page];
  const goalFact = ctx.facts.find((f) => /^goal/i.test(f.key));
  const order = [...dims].sort((a, b) => perDim[a] - perDim[b] || dims.indexOf(a) - dims.indexOf(b));
  for (const dim of order) {
    const options = TEMPLATES.map((t, i) => ({ t, i })).filter(({ t, i }) => t.dim === dim && !used.has(i) && (!t.goal || goalFact) && (!t.fact || ctx.facts.length));
    // Goal lines win on a goal fact; otherwise lines with no fact keep the number guard trivial.
    const pick = options.find(({ t }) => t.goal && goalFact) ?? options.find(({ t }) => !t.fact) ?? options[0];
    if (!pick) continue;
    const fact = goalFact ?? ctx.facts[0];
    let text = pick.t.text.replace("{fact}", fact ? factText(fact) : "");
    const days = new Set(recent.filter((e) => ctx.now - e.generatedAt < 7 * DAY_MS).map((e) => Math.floor(e.generatedAt / DAY_MS)));
    if (days.size >= 3 && sentenceCount(text) === 1 && !pick.t.fact) text = `You have kept coming back to this all week. ${text}`;
    if (validateLift(text, ctx.facts).ok) return text;
  }
  return null;
}
