import type { AmaraTierInfo, TerrenceAction, TerrenceInteraction } from "../types";
import { routeInfoFrom } from "./routeInfo";

export type ConversationMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: string;
  status?: "failed";
  model?: string;
  /** Model id the Amara Router reports it served this reply with (routeInfo.ts); absent = unknown. */
  servedModel?: string;
  /** Router tier for Amara's reply; absent = unknown (no badge). */
  tier?: AmaraTierInfo;
  /** "home": answered by Home Assistant's conversation agent (chat pre-route), not a model. */
  source?: "home";
};

export const CONVERSATION_KEY = "nexus.terrence.conversationId";
export const HERMES_SESSION_KEY = "nexus.terrence.hermesSessionId";
export const HERMES_SESSION_PROFILE_KEY = "nexus.terrence.hermesSessionProfile";
export const NEXUS_RUNTIME_PROFILE = "nexus-managed-actions-v5";

export function createConversationId() {
  return typeof crypto?.randomUUID === "function"
    ? `nexus-${crypto.randomUUID()}`
    : `nexus-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function displayAssistantMessage(content: string) {
  return content.replace(/^\u{256d}[^\n]*Hermes[^\n]*\r?\n/u, "").trim();
}

export function actionPlan(action: TerrenceAction): string[] {
  if (action.toolId === "macos.workflow") {
    const steps = action.parameters.steps;
    if (!Array.isArray(steps)) return [];
    return steps.flatMap((step, index) => {
      if (!step || typeof step !== "object") return [];
      const values = step as Record<string, unknown>;
      const kind = typeof values.kind === "string" ? values.kind.replace(/_/g, " ") : "desktop step";
      const target = typeof values.url === "string" ? values.url
        : typeof values.application === "string" ? values.application
          : typeof values.path === "string" ? values.path
            : typeof values.from === "string" && typeof values.to === "string" ? `${values.from} to ${values.to}`
              : typeof values.from === "string" && typeof values.name === "string" ? `${values.from} to ${values.name}`
                : typeof values.name === "string" ? values.name
                  : typeof values.volume === "number" ? `${values.volume}%`
                    : typeof values.delta === "number" ? `${values.delta > 0 ? "+" : ""}${values.delta}%`
                      : typeof values.muted === "boolean" ? values.muted ? "on" : "off"
                        : "";
      return [`${index + 1}. ${kind}${target ? `: ${target}` : ""}`];
    });
  }
  if (action.toolId === "filesystem.organize_directory") {
    const moves = action.parameters.moves;
    if (!Array.isArray(moves)) return [];
    return moves.flatMap((move, index) => {
      if (!move || typeof move !== "object") return [];
      const values = move as Record<string, unknown>;
      const source = typeof values.from === "string" ? values.from.split("/").filter(Boolean).pop() : null;
      const category = typeof values.category === "string" ? values.category : "organized files";
      return source ? [`${index + 1}. Move ${source} to ${category}`] : [];
    });
  }
  return [];
}

export function messagesFromInteractions(interactions: TerrenceInteraction[]): ConversationMessage[] {
  return interactions.flatMap((interaction) => {
    const route = routeInfoFrom(interaction);
    const messages: ConversationMessage[] = [{
      id: `${interaction.id}-user`,
      role: "user",
      content: interaction.userMessage,
      timestamp: interaction.startedAt,
    }];
    if (interaction.assistantMessage) {
      messages.push({
        id: `${interaction.id}-assistant`,
        role: "assistant",
        content: displayAssistantMessage(interaction.assistantMessage),
        timestamp: interaction.completedAt,
        model: interaction.model,
        ...(route.tier ? { tier: route.tier } : {}),
        ...(route.model ? { servedModel: route.model } : {}),
        ...(interaction.source === "home" ? { source: "home" as const } : {}),
      });
    } else if (interaction.error) {
      messages.push({
        id: `${interaction.id}-failed`,
        role: "assistant",
        content: interaction.error,
        timestamp: interaction.completedAt,
        status: "failed",
      });
    }
    return messages;
  });
}
