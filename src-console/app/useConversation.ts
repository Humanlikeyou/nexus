import { useCallback, useEffect, useState } from "react";
import { interactWithTerrence, listTerrenceInteractions } from "../bridge";
import { type ConversationMessage, CONVERSATION_KEY, HERMES_SESSION_KEY, HERMES_SESSION_PROFILE_KEY, NEXUS_RUNTIME_PROFILE, createConversationId, messagesFromInteractions } from "./conversation";

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
  const [sending, setSending] = useState(false);
  useEffect(() => {
    let activeRequest = true;
    listTerrenceInteractions(conversationId).then((interactions) => {
      if (!activeRequest) return;
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
  const sendToTerrence = useCallback(async (message: string) => {
    const requestId = `pending-${Date.now()}`;
    setMessages((current) => [...current, { id: `${requestId}-user`, role: "user", content: message, timestamp: new Date().toISOString() }]);
    setSending(true);
    try {
      const interaction = await interactWithTerrence({ message, conversationId, hermesSessionId });
      if (interaction.hermesSessionId) {
        setHermesSessionId(interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_KEY, interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_PROFILE_KEY, interaction.runtimeProfile);
      }
      setMessages((current) => [...current, ...messagesFromInteractions([interaction]).filter((entry) => entry.role === "assistant")]);
      await refresh();
    } catch (error) {
      setMessages((current) => [...current, {
        id: `${requestId}-failed`,
        role: "assistant",
        content: error instanceof Error ? error.message : String(error),
        timestamp: new Date().toISOString(),
        status: "failed",
      }]);
    } finally {
      setSending(false);
    }
  }, [conversationId, hermesSessionId, refresh]);
  const startNewConversation = useCallback(() => {
    const created = createConversationId();
    window.localStorage.setItem(CONVERSATION_KEY, created);
    window.localStorage.removeItem(HERMES_SESSION_KEY);
    window.localStorage.removeItem(HERMES_SESSION_PROFILE_KEY);
    setHermesSessionId(null);
    setMessages([]);
    setConversationId(created);
  }, []);
  return { conversationId, messages, sending, sendToTerrence, startNewConversation };
}
