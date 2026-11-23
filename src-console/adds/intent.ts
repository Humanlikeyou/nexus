import type { AddKind } from "./adapters";

/**
 * Deterministic add-intent detection (HANDOFF-1111v §5), kept pure and separate so decision 0016's single Rust turn
 * pipeline can plug it in. Explicit phrasings only; anything else is null and nothing happens.
 */
export type AddVia = "chat" | "voice";
export type AddIntent = { kind: AddKind; text: string };

const LEAD = "(?:(?:hey |ok |okay )?amara[,:]?\\s+)?(?:please\\s+)?";
const BODY = "[\\s,:]+(\\S.{2,})$";
const RULES: { kind: AddKind; re: RegExp }[] = [
  { kind: "goal", re: new RegExp(`^${LEAD}add (?:a |the )?(?:new )?(?:goal|habit)(?: to| that| of| for)?${BODY}`, "i") },
  { kind: "date", re: new RegExp(`^${LEAD}remind me (?:on|by)${BODY}`, "i") },
  { kind: "date", re: new RegExp(`^${LEAD}add (?:a |the )?(?:date|deadline)(?: for)?${BODY}`, "i") },
  { kind: "task", re: new RegExp(`^${LEAD}add (?:a |the )?(?:enterprise )?task(?: to| for)?${BODY}`, "i") },
  { kind: "idea", re: new RegExp(`^${LEAD}(?:add (?:a |the )?)?content idea(?: for)?${BODY}`, "i") },
];

/** Explicit phrasings only. "add a goal …", "remind me on <date> …", "add task …", "content idea: …". */
export function detectAddIntent(input: string): AddIntent | null {
  const t = input.trim().replace(/\s+/g, " ");
  if (t.length > 500) return null;
  for (const { kind, re } of RULES) {
    const m = re.exec(t);
    if (m) return { kind, text: m[1].replace(/[.!?]+$/, "").trim() };
  }
  return null;
}

const CONFIRM = new RegExp(`^${LEAD}(?:yes[, ]+)?(?:confirm|save it|confirm save|save)(?: it| that| this)?[.!]?$`, "i");
const CANCEL = new RegExp(`^${LEAD}(?:cancel|never ?mind|discard|don'?t save(?: it)?)[.!]?$`, "i");
export const isConfirm = (s: string) => CONFIRM.test(s.trim());
export const isCancel = (s: string) => CANCEL.test(s.trim());

