import { addAdapter, habitDoneAdapter } from "./adapters";
import type { AddAdapter, AddPreview } from "./flow";

/**
 * HANDOFF-1111v §5: chat and voice call the SAME add adapter as the page composers (add_preview, then add_save with
 * via "chat" | "voice"). There is no second write path. Intent detection is deterministic and anchored to explicit
 * phrasings; anything else returns null and nothing happens. A preview is never saved without an explicit confirm turn.
 * Audio never reaches this module: voice arrives as the transcript text amara-voice already logged (turns.jsonl).
 */
import { detectAddIntent, isCancel, isConfirm, type AddIntentKind, type AddVia } from "./intent";
export { detectAddIntent, isCancel, isConfirm, type AddIntent, type AddVia } from "./intent";

const LABEL: Record<AddIntentKind, string> = { goal: "goal", date: "date", task: "task", idea: "content idea", "habit-done": "habit" };
export const PENDING_MS = 2 * 60_000;

const lines = (kind: AddIntentKind, p: AddPreview) =>
  `Check before saving (${LABEL[kind]}): ${p.title}. ${p.fields.map(([k, v]) => `${k}: ${v}`).join(". ")}.${p.note ? ` ${p.note}` : ""} Say "confirm" to save or "cancel".`;

type Pending = { kind: AddIntentKind; preview: AddPreview; via: AddVia; at: number };
export type ChatAddDeps = { adapter?: (kind: AddIntentKind) => AddAdapter; now?: () => number };

/** One conversation's add flow: detect, preview (nothing written), then a separate confirm turn to save. */
export function createChatAdds(deps: ChatAddDeps = {}) {
  const adapter = deps.adapter ?? ((k: AddIntentKind) => (k === "habit-done" ? habitDoneAdapter() : addAdapter(k)));
  const now = deps.now ?? (() => Date.now());
  let pending: Pending | null = null;
  /** Monotonic: a slower, older turn can never assign or save over a newer one. */
  let seq = 0;

  /** null = not an add turn (let chat handle it). A string = Amara's local reply for this turn. */
  async function handle(text: string, via: AddVia): Promise<string | null> {
    const live = pending && now() - pending.at <= PENDING_MS ? pending : null;
    if (!live) pending = null;
    if (live && isConfirm(text)) {
      pending = null;
      seq++; // any parse still in flight is now stale and can't re-arm a pending save
      try {
        const r = await adapter(live.kind).save(live.preview, live.via);
        if (r.state === "unwired") return "This needs wiring, so nothing was saved.";
        return `Saved: ${r.value.title}. You can undo it from the page for 8 seconds, and it is in History as from ${live.via}.`;
      } catch { return "Couldn't save, so nothing was added. Say the add again to retry."; }
    }
    if (live && isCancel(text)) { pending = null; seq++; return "Cancelled. Nothing was saved."; }
    const intent = detectAddIntent(text);
    if (!intent) return null;
    pending = null;
    const mine = ++seq;
    try {
      const r = await adapter(intent.kind).parse(intent.text);
      if (mine !== seq) return "A newer request replaced this one. Nothing was saved.";
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
