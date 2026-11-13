import { addAdapter, type AddKind } from "./adapters";
import type { AddAdapter, AddPreview } from "./flow";

/**
 * HANDOFF-1111v §5: chat and voice call the SAME add adapter as the page composers (add_preview, then add_save with
 * via "chat" | "voice"). There is no second write path. Intent detection is deterministic and anchored to explicit
 * phrasings; anything else returns null and nothing happens. A preview is never saved without an explicit confirm turn.
 * Audio never reaches this module: voice arrives as the transcript text amara-voice already logged (turns.jsonl).
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

const CONFIRM = /^(?:yes[, ]+)?(?:confirm|save it|confirm save|save)(?: it| that| this)?[.!]?$/i;
const CANCEL = /^(?:cancel|never ?mind|discard|don'?t save(?: it)?)[.!]?$/i;
export const isConfirm = (s: string) => CONFIRM.test(s.trim());
export const isCancel = (s: string) => CANCEL.test(s.trim());

const LABEL: Record<AddKind, string> = { goal: "goal", date: "date", task: "task", idea: "content idea" };
export const PENDING_MS = 2 * 60_000;

const lines = (kind: AddKind, p: AddPreview) =>
  `Check before saving (${LABEL[kind]}): ${p.title}. ${p.fields.map(([k, v]) => `${k}: ${v}`).join(". ")}.${p.note ? ` ${p.note}` : ""} Say "confirm" to save or "cancel".`;

type Pending = { kind: AddKind; preview: AddPreview; via: AddVia; at: number };
export type ChatAddDeps = { adapter?: (kind: AddKind) => AddAdapter; now?: () => number };

/** One conversation's add flow: detect, preview (nothing written), then a separate confirm turn to save. */
export function createChatAdds(deps: ChatAddDeps = {}) {
  const adapter = deps.adapter ?? ((k: AddKind) => addAdapter(k));
  const now = deps.now ?? (() => Date.now());
  let pending: Pending | null = null;

  /** null = not an add turn (let chat handle it). A string = Amara's local reply for this turn. */
  async function handle(text: string, via: AddVia): Promise<string | null> {
    const live = pending && now() - pending.at <= PENDING_MS ? pending : null;
    if (!live) pending = null;
    if (live && isConfirm(text)) {
      pending = null;
      try {
        const r = await adapter(live.kind).save(live.preview, live.via);
        if (r.state === "unwired") return "This needs wiring, so nothing was saved.";
        return `Saved: ${r.value.title}. You can undo it from the page for 8 seconds, and it is in History as from ${live.via}.`;
      } catch { return "Couldn't save, so nothing was added. Say the add again to retry."; }
    }
    if (live && isCancel(text)) { pending = null; return "Cancelled. Nothing was saved."; }
    const intent = detectAddIntent(text);
    if (!intent) return null;
    pending = null;
    try {
      const r = await adapter(intent.kind).parse(intent.text);
      if (r.state === "unwired") return "Adding this needs wiring, so nothing was saved.";
      const v = r.value;
      if (v.kind === "ask") return `${v.question}${v.note ? ` ${v.note}` : ""} Say the add again with the answer. Nothing was saved.`;
      if (v.kind === "elsewhere") return `${v.message} Nothing was saved.`;
      pending = { kind: intent.kind, preview: v.preview, via, at: now() };
      return lines(intent.kind, v.preview);
    } catch { return "Couldn't read that: Amara isn't reachable. Nothing was saved."; }
  }
  return { handle, hasPending: () => pending !== null && now() - pending.at <= PENDING_MS };
}
