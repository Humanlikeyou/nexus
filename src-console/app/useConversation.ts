import { useCallback, useEffect, useRef, useState } from "react";
import { interactWithTerrence, listTerrenceInteractions, prepareAmaraTurn, recordAmaraInteraction } from "../bridge";
import { deliberate } from "@/terrance/system/deliberation";
import { resetCompressor } from "@/terrance/system/contextCompressor";
import { type ConversationMessage, CONVERSATION_KEY, HERMES_SESSION_KEY, HERMES_SESSION_PROFILE_KEY, NEXUS_RUNTIME_PROFILE, createConversationId, messagesFromInteractions } from "./conversation";
import { HOME_SOURCE, dropLegacyHomeReplyIds, preRouteHome } from "../lib/homeAssistant";
import { compressTurns, formatDeliberation, guardSections, historyTurns, parseChatCommand, tierFromRouter } from "./amaraChat";

export interface SendOptions {
  /** Composer Deep toggle. */
  deep?: boolean;
}

/** Background context for a message, through the injection guard. Never blocks a send. */
async function guardedContext(message: string) {
  try {
    const prepared = await prepareAmaraTurn(message);
    const guarded = guardSections(prepared.sections);
    if (guarded.threats.length > 0) console.warn("[amara] injection guard redacted context:", guarded.threats.join(", "));
    return { sections: guarded.sections, itemIds: prepared.itemIds };
  } catch (error) {
    console.warn("[amara] turn context unavailable", error);
    return { sections: [], itemIds: [] };
  }
}

/** Amara chat state: conversation id, Hermes session, transcript, send / new conversation. */
export function useConversation(refresh: () => Promise<void>) {
  const [conversationId, setConversationId] = useState(() => {
    const existing = window.localStorage.getItem(CONVERSATION_KEY);
    if (existing) return existing;
    const created = createConversationId();
    window.localStorage.setItem(CONVERSATION_KEY, created);
    return created;
  });
  const [hermesSessionId, setHermesSessionId] = useState<string | null>(() => (
    window.localStorage.getItem(HERMES_SESSION_PROFILE_KEY) === NEXUS_RUNTIME_PROFILE
      ? window.localStorage.getItem(HERMES_SESSION_KEY)
      : null
  ));
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const messagesRef = useRef<ConversationMessage[]>([]);
  useEffect(() => { messagesRef.current = messages; }, [messages]);
  const [sending, setSending] = useState(false);
  /** Bumped by each send and by cancel; a reply whose generation is stale is dropped. */
  const generation = useRef(0);
  useEffect(() => {
    let activeRequest = true;
    resetCompressor();
    listTerrenceInteractions(conversationId).then((interactions) => {
      if (!activeRequest) return;
      dropLegacyHomeReplyIds();
      setMessages(messagesFromInteractions(interactions));
      const latestSession = [...interactions]
        .reverse()
        .find((interaction) => interaction.runtimeProfile === NEXUS_RUNTIME_PROFILE && interaction.hermesSessionId)
        ?.hermesSessionId;
      if (latestSession) {
        setHermesSessionId(latestSession);
        window.localStorage.setItem(HERMES_SESSION_KEY, latestSession);
        window.localStorage.setItem(HERMES_SESSION_PROFILE_KEY, NEXUS_RUNTIME_PROFILE);
      }
    }).catch((error) => console.warn("[console] interaction history unavailable", error));
    return () => { activeRequest = false; };
  }, [conversationId]);

  const runDeliberation = useCallback(async (message: string, topic: string, requestId: string, live: () => boolean) => {
    const started = Date.now();
    const context = await guardedContext(topic);
    let routerError = "";
    const result = await deliberate(topic, context.sections.map((s) => s.text).join("\n\n") || undefined, {
      deep: true,
      onError: (text) => { routerError = text; },
    });
    if (!result) throw new Error(routerError || "Amara couldn't finish the deliberation. Check that the Amara Router is running.");
    const content = formatDeliberation(topic, result);
    const tier = tierFromRouter(result.tier, result.routingReason);
    const saved = await recordAmaraInteraction({ conversationId, userMessage: message, assistantMessage: content, tier, durationMs: Date.now() - started })
      .catch((error) => { console.warn("[amara] deliberation not saved", error); return null; });
    if (!live()) return;
    setMessages((current) => [...current, {
      id: saved ? `${saved.id}-assistant` : `${requestId}-assistant`,
      role: "assistant",
      content,
      timestamp: new Date().toISOString(),
      ...(tier ? { tier } : {}),
    }]);
  }, [conversationId]);

  const sendToTerrence = useCallback(async (message: string, options: SendOptions = {}) => {
    const requestId = `pending-${Date.now()}`;
    const gen = ++generation.current;
    const live = () => generation.current === gen;
    const command = parseChatCommand(message, options.deep ?? false);
    const history = historyTurns(messagesRef.current);
    setMessages((current) => [...current, { id: `${requestId}-user`, role: "user", content: message, timestamp: new Date().toISOString() }]);
    setSending(true);
    try {
      if (command.kind === "deliberate") {
        if (!command.topic) throw new Error("Add a topic: /deliberate <topic>");
        await runDeliberation(message, command.topic, requestId, live);
        return;
      }
      // Home Assistant pre-route for plain chat only: Deep (toggle or /deep) and /deliberate go straight to Hermes.
      // NoMatch or any HA error falls through to Hermes too.
      const home = command.deep ? { handled: false as const } : await preRouteHome(command.text);
      if (home.handled) {
        const started = Date.now();
        const saved = await Promise.resolve()
          .then(() => recordAmaraInteraction({ conversationId, userMessage: message, assistantMessage: home.speech, tier: null, durationMs: Date.now() - started, source: HOME_SOURCE }))
          .catch(() => { console.warn("[ha] home reply not saved"); return null; });
        if (!live()) return;
        setMessages((current) => [...current, { id: saved ? `${saved.id}-assistant` : `${requestId}-home`, role: "assistant", content: home.speech, timestamp: new Date().toISOString(), source: "home" }]);
        return;
      }
      const [context, compressed] = await Promise.all([
        guardedContext(command.text),
        compressTurns(history).catch((error) => { console.warn("[amara] compression skipped", error); return { turns: history, compressed: false }; }),
      ]);
      const interaction = await interactWithTerrence({
        message: command.text,
        conversationId,
        hermesSessionId,
        history: compressed.turns,
        deep: command.deep,
        context: context.sections,
        contextItemIds: context.itemIds,
      });
      if (!live()) return;
      if (interaction.hermesSessionId) {
        setHermesSessionId(interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_KEY, interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_PROFILE_KEY, interaction.runtimeProfile);
      }
      setMessages((current) => [...current, ...messagesFromInteractions([interaction]).filter((entry) => entry.role === "assistant")]);
      await refresh();
    } catch (error) {
      if (!live()) return;
      setMessages((current) => [...current, {
        id: `${requestId}-failed`,
        role: "assistant",
        content: error instanceof Error ? error.message : String(error),
        timestamp: new Date().toISOString(),
        status: "failed",
      }]);
    } finally {
      if (live()) setSending(false);
    }
  }, [conversationId, hermesSessionId, refresh, runDeliberation]);
  /** Stop waiting: the pending reply is ignored when it lands (the backend call itself can't be aborted). */
  const cancelPending = useCallback(() => {
    generation.current++;
    setSending(false);
  }, []);
  const startNewConversation = useCallback(() => {
    generation.current++;
    setSending(false);
    const created = createConversationId();
    window.localStorage.setItem(CONVERSATION_KEY, created);
    window.localStorage.removeItem(HERMES_SESSION_KEY);
    window.localStorage.removeItem(HERMES_SESSION_PROFILE_KEY);
    resetCompressor();
    setHermesSessionId(null);
    setMessages([]);
    setConversationId(created);
  }, []);
  return { conversationId, messages, sending, sendToTerrence, startNewConversation, cancelPending };
}
