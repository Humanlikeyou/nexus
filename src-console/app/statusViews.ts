import type { KbStatus, OpenClawStatus } from "../types";
import type { Tone } from "../ui";
import { openClawBadge } from "./openclaw";

/** Fixed copy for each knowledge-base state. Backend text (detail, errors) is never shown. */
export interface KbView { tone: Tone; label: string; body: string; retry: boolean }

export function kbView(kb: KbStatus, unavailable: boolean): KbView {
  if (unavailable) return { tone: "warning", label: "Unavailable", body: "status unavailable", retry: true };
  switch (kb.state) {
    case "running": {
      const docs = kb.docs ?? 0;
      const parts = [`${docs} ${docs === 1 ? "document" : "documents"}`, kb.model].filter(Boolean);
      return { tone: "success", label: "Ready", body: parts.join(" · "), retry: false };
    }
    case "empty": return { tone: "neutral", label: "Empty", body: "knowledge base is empty", retry: false };
    case "not_running": return { tone: "warning", label: "Unavailable", body: "not running", retry: true };
    default: return { tone: "danger", label: "Error", body: "query failed", retry: true };
  }
}

/**
 * Assistant row for Home and Chat. Ambiguous (loading, status unreadable): neutral "Assistant".
 * Gateway reachable: "Amara". Installed but silent: "Assistant backend not reachable"
 * (Unavailable + Retry, warning). Not installed / offline (local mode): hidden.
 */
export type AssistantView =
  | { kind: "row"; title: string; tone: Tone; label: string; subtitle: string }
  | { kind: "unreachable"; message: string }
  | null;

export const BACKEND_UNREACHABLE_COPY = "Assistant backend not reachable";

export function assistantView(s: OpenClawStatus, opts: { loading: boolean; unavailable: boolean }): AssistantView {
  if (opts.loading || opts.unavailable) return { kind: "row", title: "Assistant", tone: "neutral", label: "Checking", subtitle: "Backend status unknown" };
  const status = s.status ?? (s.online ? "online" : "offline");
  if (status === "online") return { kind: "row", title: "Amara", tone: "success", label: "Online", subtitle: openClawBadge(s).subtitle === "Gateway reachable" ? "Backend reachable" : openClawBadge(s).subtitle };
  if (status === "configured") return { kind: "unreachable", message: BACKEND_UNREACHABLE_COPY };
  return null;
}

/** crew_tools rows: fixed per-tool limits from the crew registry (15 s timeout, 4000-char cap). */
export const CREW_TOOL_TIMEOUT_S = 15;
export const CREW_TOOL_OUTPUT_CAP = 4000;
