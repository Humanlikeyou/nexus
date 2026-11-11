import type { KbStatus } from "../types";
import type { Tone } from "../ui";

/** Fixed copy for each knowledge-base state. Backend text (detail, errors) is never shown. */
export interface KbView { tone: Tone; label: string; body: string; retry: boolean; title?: string }

export function kbView(kb: KbStatus, unavailable: boolean): KbView {
  if (unavailable) return { tone: "warning", label: "Unavailable", body: "status unavailable", retry: true };
  switch (kb.state) {
    case "running": {
      const docs = kb.docs ?? 0;
      const parts = [`${docs} ${docs === 1 ? "document" : "documents"}`, kb.model].filter(Boolean);
      return { tone: "success", label: "Ready", body: parts.join(" · "), retry: false };
    }
    case "empty": return { tone: "neutral", label: "Empty", title: "Knowledge base is empty", body: "Add documents to search them.", retry: false };
    case "not_running": return { tone: "warning", label: "Unavailable", body: "not running", retry: true };
    default: return { tone: "danger", label: "Error", body: "query failed", retry: true };
  }
}

/** crew_tools rows: fixed per-tool limits from the crew registry (15 s timeout, 4000-char cap). */
export const CREW_TOOL_TIMEOUT_S = 15;
export const CREW_TOOL_OUTPUT_CAP = 4000;
