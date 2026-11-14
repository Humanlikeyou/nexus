/**
 * Amara chat features on top of Hermes (Hermes stays the only identity and memory):
 * - /deliberate <topic> → deliberation.ts (optimist, skeptic, synthesis) on the router's deep tier
 * - /deep or the composer toggle → Deep through the router
 * - injectionGuard.ts on ingested/external content before it reaches a prompt
 * - contextCompressor.ts on long histories before they are sent
 */
import { scanForInjection } from "../legacy-core/terrance/system/injectionGuard";
import { compressIfNeeded, type CompressibleMessage } from "../legacy-core/terrance/system/contextCompressor";
import type { DeliberationResult } from "../legacy-core/terrance/system/deliberation";
import type { RouterResponse } from "../legacy-core/lib/types";
import type { AmaraContextSection, AmaraTierInfo, ChatTurn } from "../types";
import type { ConversationMessage } from "./conversation";

export type ChatCommand =
  | { kind: "deliberate"; topic: string }
  | { kind: "chat"; text: string; deep: boolean };

/** `/deliberate <topic>`; `/deep …` sets deep (the prefix itself is kept for the router). */
export function parseChatCommand(text: string, deepToggle: boolean): ChatCommand {
  const trimmed = text.trim();
  const deliberate = /^\/deliberate(?:\s+([\s\S]*))?$/i.exec(trimmed);
  if (deliberate) return { kind: "deliberate", topic: (deliberate[1] ?? "").trim() };
  return { kind: "chat", text: trimmed, deep: deepToggle || /^\/deep(?:\s|$)/i.test(trimmed) };
}

/** Injection guard over text from outside the composer. High-severity matches are redacted. */
export function guardExternal(text: string): { text: string; threats: string[] } {
  const result = scanForInjection(text);
  return { text: result.sanitized, threats: result.threats.map((t) => t.pattern) };
}

/** Guard every background section (Nexus context, projects, health, …) before it is sent. */
export function guardSections(sections: AmaraContextSection[]): { sections: AmaraContextSection[]; threats: string[] } {
  const threats: string[] = [];
  const guarded = sections.map((s) => {
    const g = guardExternal(s.text);
    threats.push(...g.threats.map((t) => `${s.tag}:${t}`));
    return { tag: s.tag, text: g.text };
  });
  return { sections: guarded, threats };
}

/**
 * The visible conversation as Hermes history. Failed bubbles are left out. Amara's earlier
 * replies can carry tool or web content, so they pass the guard; Taisun's own words do not.
 */
export function historyTurns(messages: ConversationMessage[]): ChatTurn[] {
  return messages
    .filter((m) => m.status !== "failed" && !m.local && m.content.trim() !== "")
    .map((m) => ({ role: m.role, content: m.role === "assistant" ? guardExternal(m.content).text : m.content }));
}

/** contextCompressor.ts over the history; its summary comes back as one assistant note. */
export async function compressTurns(turns: ChatTurn[], compress = compressIfNeeded): Promise<{ turns: ChatTurn[]; compressed: boolean }> {
  const now = Date.now();
  const input: CompressibleMessage[] = turns.map((t, i) => ({ role: t.role === "user" ? "user" : "terrance", text: t.content, ts: now + i }));
  const result = await compress(input);
  if (!result.compressed) return { turns, compressed: false };
  // A system-role summary would change Hermes's session id (it hashes the system prompt), so it
  // travels as an assistant note between the protected head and tail.
  return {
    turns: result.messages.map((m) => ({ role: m.role === "user" ? "user" : "assistant", content: m.text })),
    compressed: true,
  };
}

/** Router tier → badge label. Unknown → null (no badge). */
export function tierLabel(tier: AmaraTierInfo | null | undefined): "Fast" | "Deep" | "Claude" | null {
  switch (tier?.tier) {
    case "fast": return "Fast";
    case "deep": return "Deep";
    case "claude": return "Claude";
    default: return null;
  }
}

const SOURCE_TEXT: Record<string, string> = {
  hermes_metadata: "from the router's reply metadata",
  router_log: "from the Amara Router's route log for this turn",
  router_dry_run: "the Amara Router's routing decision for this message",
  router_reply: "from the Amara Router's reply metadata",
};

/** Tooltip for the badge: where the tier came from and the router's reason. */
export function tierTitle(tier: AmaraTierInfo): string {
  const source = SOURCE_TEXT[tier.source] ?? tier.source;
  return [`${tierLabel(tier)} tier, ${source}`, tier.reason && `reason: ${tier.reason}`].filter(Boolean).join(" · ");
}

/** The app's tier name (light/deep/claude) from chat_with_router → router names. */
export function tierFromRouter(tier: RouterResponse["tier"] | undefined, reason = ""): AmaraTierInfo | null {
  if (!tier) return null;
  return { tier: tier === "light" ? "fast" : tier, reason, source: "router_reply" };
}

/** The chat bubble for a deliberation. */
export function formatDeliberation(topic: string, result: DeliberationResult): string {
  const confidence = result.confidence[0].toUpperCase() + result.confidence.slice(1);
  return [
    `Deliberation: ${topic}`,
    result.synthesis.trim(),
    `Optimist\n${result.bullCase.trim()}`,
    `Skeptic\n${result.bearCase.trim()}`,
    `Confidence: ${confidence}`,
  ].join("\n\n");
}
