import { useCallback, useEffect, useMemo, useRef } from "react";
import type { ConversationMessage } from "../app/conversation";
import type { VoiceTurn } from "../types";
import { createTurnTracker, spokenText } from "../hooks/freshVoiceTurns";
import { createChatAdds } from "./chatAdds";

type Turn = VoiceTurn;
type Post = (messages: ConversationMessage[]) => void;

/**
 * Home chat + voice call sites for adds (HANDOFF-1111v §5). `handleTyped` returns true when the turn was an add turn
 * (so it is not also sent to Hermes). Voice turns are the transcripts amara-voice already logged; the first poll is
 * historical and skipped, like the Navy and window prompts. Both go through the one adds adapter.
 */
export function useChatAdds(post: Post, turns: Turn[]) {
  const adds = useMemo(() => createChatAdds(), []);
  const handleTyped = useCallback(async (message: string): Promise<boolean> => {
    const reply = await adds.handle(message, "chat");
    if (reply == null) return false;
    const at = new Date().toISOString();
    const id = `add-${Date.now()}`;
    post([{ id: `${id}-user`, role: "user", content: message, timestamp: at, local: true }, { id: `${id}-assistant`, role: "assistant", content: reply, timestamp: at, local: true }]);
    return true;
  }, [adds, post]);

  const track = useRef(createTurnTracker());
  const chain = useRef<Promise<unknown>>(Promise.resolve());
  useEffect(() => {
    // Every new fresh turn, oldest first, one after another. The spoken side is already in the thread (voice turns);
    // only Amara's local reply is added.
    for (const turn of track.current(turns)) {
      const q = spokenText(turn);
      if (!q) continue;
      chain.current = chain.current.then(() => adds.handle(q, "voice")).then((reply) => {
        if (reply != null) post([{ id: `add-voice-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`, role: "assistant", content: reply, timestamp: new Date().toISOString(), local: true }]);
      }, () => undefined);
    }
  }, [turns, adds, post]);
  return { handleTyped };
}
