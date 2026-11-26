import type { AddKind } from "./adapters";

/**
 * Deterministic add-intent detection (HANDOFF-1111v §5), kept pure and separate so decision 0016's single Rust turn
 * pipeline can plug it in. Explicit phrasings only; anything else is null and nothing happens.
 */
export type AddVia = "chat" | "voice";
/** "habit-done" (0045 follow-up): "I did my stretch", "mark stretch done", "I didn't walk today". */
export type AddIntentKind = AddKind | "habit-done";
export type AddIntent = { kind: AddIntentKind; text: string };

const LEAD = "(?:(?:hey |ok |okay )?amara[,:]?\\s+)?(?:please\\s+)?";
const BODY = "[\\s,:]+(\\S.{2,})$";
const RULES: { kind: AddKind; re: RegExp }[] = [
  { kind: "goal", re: new RegExp(`^${LEAD}add (?:a |the )?(?:new )?(?:goal|habit)(?: to| that| of| for)?${BODY}`, "i") },
  { kind: "date", re: new RegExp(`^${LEAD}remind me (?:on|by)${BODY}`, "i") },
  { kind: "date", re: new RegExp(`^${LEAD}add (?:a |the )?(?:date|deadline)(?: for)?${BODY}`, "i") },
  { kind: "task", re: new RegExp(`^${LEAD}add (?:a |the )?(?:enterprise )?task(?: to| for)?${BODY}`, "i") },
  { kind: "idea", re: new RegExp(`^${LEAD}(?:add (?:a |the )?)?content idea(?: for)?${BODY}`, "i") },
];

/** Habit marking, tried after the add rules. The body goes through `habitDoneText`; `base` is the polarity of the lead words. */
const HABIT_RULES: { base: boolean; re: RegExp }[] = [
  { base: false, re: new RegExp(`^${LEAD}(?:i didn't|i didnt|i did not|i haven't|i havent|i have not|i skipped|i missed)${BODY}`, "i") },
  { base: true, re: new RegExp(`^${LEAD}(?:i did|i've done|i have done|i just did|i finished|i completed)${BODY}`, "i") },
  { base: true, re: new RegExp(`^${LEAD}(?:mark|tick off|check off)${BODY}`, "i") },
];

/**
 * "my stretch done today" -> "done today stretch". Strips a trailing day ("today" / "yesterday", optionally "for"), a trailing
 * "done" / "not done" / "undone" (optionally "as"), then a leading "do"/"done" and "my"/"the". Null when no habit words are left.
 * Format: `<done|not> <today|yesterday> <habit words>`; src-tauri adds.rs `habit_done_text` is the same, step for step.
 */
export function habitDoneText(base: boolean, body: string): string | null {
  let s = body;
  let done = base;
  let day = "today";
  const stripDay = () => { const m = /\s+(?:for\s+)?(today|yesterday)$/i.exec(s); if (m) { day = m[1].toLowerCase(); s = s.slice(0, m.index); } };
  stripDay();
  const d = /\s+(?:as\s+)?(not done|undone|done)$/i.exec(s);
  if (d) { done = d[1].toLowerCase() === "done"; s = s.slice(0, d.index); stripDay(); }
  s = s.replace(/^(?:done|do)\s+/i, "").replace(/^(?:my|the)\s+/i, "").trim();
  if (s.length < 2 || s.length > 60 || /^(?:my|the|do|done)$/i.test(s)) return null;
  return `${done ? "done" : "not"} ${day} ${s}`;
}

/** Explicit phrasings only. "add a goal …", "remind me on <date> …", "add task …", "content idea: …". */
export function detectAddIntent(input: string): AddIntent | null {
  const t = input.trim().replace(/\s+/g, " ");
  if (t.length > 500) return null;
  for (const { kind, re } of RULES) {
    const m = re.exec(t);
    if (m) return { kind, text: m[1].replace(/[.!?]+$/, "").trim() };
  }
  for (const { base, re } of HABIT_RULES) {
    const m = re.exec(t);
    const text = m ? habitDoneText(base, m[1].replace(/[.!?]+$/, "").trim()) : null;
    if (text) return { kind: "habit-done", text };
  }
  return null;
}

const CONFIRM = new RegExp(`^${LEAD}(?:yes[, ]+)?(?:confirm|save it|confirm save|save)(?: it| that| this)?[.!]?$`, "i");
const CANCEL = new RegExp(`^${LEAD}(?:cancel|never ?mind|discard|don'?t save(?: it)?)[.!]?$`, "i");
export const isConfirm = (s: string) => CONFIRM.test(s.trim());
export const isCancel = (s: string) => CANCEL.test(s.trim());

