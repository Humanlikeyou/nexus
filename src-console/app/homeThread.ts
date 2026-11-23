import type { VoiceTurn } from "../types";
import type { ConversationMessage } from "./conversation";

/** A thread entry on the Home chat: a typed message, or one side of a spoken turn (amara-voice). */
export type HomeMessage = ConversationMessage & { voice?: boolean };

/** How many messages the collapsed Home chat shows (design-1110b A1). */
export const HOME_COLLAPSED_MESSAGES = 3;

const sentence = (s: string) => (s ? s.charAt(0).toUpperCase() + s.slice(1) : s);

/**
 * design-1110b A2: voice and typed turns share one thread. Spoken turns come from logs/turns.jsonl
 * (voice_recent_turns): the heard text is Taisun's side, the reply is Amara's. Turns that weren't
 * acted on (ignored, or with no text) are left out. Oldest first; typed messages keep their order.
 */
export function withVoiceTurns(messages: ConversationMessage[], turns: VoiceTurn[]): HomeMessage[] {
  const spoken: HomeMessage[] = [];
  turns.forEach((turn, i) => {
    const said = (turn.text || turn.heard || "").trim();
    if (turn.route === "ignored" || !said || !turn.time || !Number.isFinite(Date.parse(turn.time))) return;
    spoken.push({ id: `voice-${turn.time}-${i}-u`, role: "user", content: sentence(said), timestamp: turn.time, voice: true });
    if (turn.reply?.trim()) spoken.push({ id: `voice-${turn.time}-${i}-a`, role: "assistant", content: turn.reply.trim(), timestamp: turn.time, voice: true, ...(turn.error ? { status: "failed" as const } : {}) });
  });
  if (spoken.length === 0) return messages;
  const out: HomeMessage[] = [...messages];
  for (const m of spoken) {
    const at = Date.parse(m.timestamp);
    let i = out.length;
    while (i > 0 && Date.parse(out[i - 1].timestamp) > at) i--;
    out.splice(i, 0, m);
  }
  return out;
}

/** The assistant message being spoken: the newest assistant reply while the voice loop is speaking. */
export function speakingIndex(messages: HomeMessage[], speaking: boolean): number {
  if (!speaking) return -1;
  for (let i = messages.length - 1; i >= 0; i--) if (messages[i].role === "assistant") return i;
  return -1;
}
