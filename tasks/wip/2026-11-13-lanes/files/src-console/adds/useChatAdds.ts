import { useCallback, useEffect, useMemo, useRef } from "react";
import type { ConversationMessage } from "../app/conversation";
import { createChatAdds } from "./chatAdds";

type Turn = { time?: string; heard?: string; text?: string };
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
    post([{ id: `${id}-user`, role: "user", content: message, timestamp: at }, { id: `${id}-assistant`, role: "assistant", content: reply, timestamp: at }]);
    return true;
  }, [adds, post]);

  const primed = useRef(false);
  const seen = useRef("");
  useEffect(() => {
    const last = turns.length ? turns[turns.length - 1] : undefined;
    const key = last ? `${last.time ?? ""}:${last.heard ?? last.text ?? ""}` : "";
    if (!primed.current) { primed.current = true; seen.current = key; return; }
    if (!last || seen.current === key) return;
    seen.current = key;
    const q = (last.heard || last.text || "").trim();
    if (!q) return;
    // The spoken side is already in the thread (voice turns); only Amara's local reply is added.
    void adds.handle(q, "voice").then((reply) => {
      if (reply != null) post([{ id: `add-voice-${Date.now()}`, role: "assistant", content: reply, timestamp: new Date().toISOString() }]);
    });
  }, [turns, adds, post]);
  return { handleTyped };
}
