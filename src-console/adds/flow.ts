import type { Route } from "../app/routes";
import type { Wired } from "../lib/sampleAdapters";

/**
 * design-1111v §5: one add pattern on every page. Input → working line (≤100 ms) → preview → confirm →
 * saved with Undo for 8 s and an "Also shows in … ›" link → History. Chat and voice call the same adapter
 * and render the same preview. Pure state machine; the component in Composer.tsx only renders it.
 */
export const UNDO_MS = 8_000;

export type PreviewField = [label: string, value: string];
export type AddPreview = { title: string; fields: PreviewField[]; note?: string; /** What the backend saves; opaque, handed back unchanged. */ payload?: unknown };
/** What reading the text gave: a preview to check, a question (never a guessed legal date), or "this lives elsewhere". */
export type ParseResult =
  | { kind: "preview"; preview: AddPreview }
  | { kind: "ask"; question: string; options: string[]; note?: string }
  | { kind: "elsewhere"; message: string; link: Route; linkLabel: string };
export type Saved = { id: string; title: string; also?: { label: string; link: Route } };
export type HistoryItem = { id: string; when: string; text: string; via: "typed" | "voice" | "chat" | "Weekly review" };

/** The one write path per page (HANDOFF-1111v §5). Unwired until the backend command exists. */
export interface AddAdapter {
  /** Shown in "Needs wiring · <command>" while unwired. */
  command: string;
  wired: boolean;
  parse(text: string): Promise<Wired<ParseResult>>;
  /** `via` records how it was added in History; the page composer is "typed". */
  save(preview: AddPreview, via?: HistoryItem["via"]): Promise<Wired<Saved>>;
  undo(id: string): Promise<Wired<{ ok: true }>>;
  history(): Wired<HistoryItem[]>;
}

export type FlowState =
  | { phase: "idle"; text: string }
  | { phase: "working"; text: string }
  | { phase: "preview"; text: string; preview: AddPreview }
  | { phase: "saving"; text: string; preview: AddPreview }
  | { phase: "ask"; text: string; question: string; options: string[]; note?: string }
  | { phase: "elsewhere"; text: string; message: string; link: Route; linkLabel: string }
  | { phase: "saved"; text: string; saved: Saved; at: number }
  | { phase: "undone"; text: string; title: string }
  | { phase: "error"; text: string; message: string };

export type FlowEvent =
  | { type: "type"; text: string }
  | { type: "submit" }
  | { type: "parsed"; result: Wired<ParseResult> }
  | { type: "failed"; message: string }
  | { type: "answer"; option: string }
  | { type: "confirm" }
  | { type: "saved"; result: Wired<Saved>; at: number }
  | { type: "edit" }
  | { type: "cancel" }
  | { type: "undone" }
  | { type: "expire" };

export const IDLE: FlowState = { phase: "idle", text: "" };
const UNWIRED_MSG = "This needs wiring, so nothing was saved. Your text is kept.";

export function flow(state: FlowState, event: FlowEvent): FlowState {
  switch (event.type) {
    case "type":
      return state.phase === "working" || state.phase === "saving" ? state : { phase: "idle", text: event.text };
    case "submit":
      if (state.phase !== "idle" && state.phase !== "error" && state.phase !== "saved" && state.phase !== "undone") return state;
      return state.text.trim() ? { phase: "working", text: state.text } : state;
    case "answer":
      if (state.phase !== "ask") return state;
      return { phase: "working", text: `${state.text} (${event.option})` };
    case "parsed": {
      if (state.phase !== "working") return state;
      const r = event.result;
      if (r.state === "unwired") return { phase: "error", text: state.text, message: UNWIRED_MSG };
      const v = r.value;
      if (v.kind === "preview") return { phase: "preview", text: state.text, preview: v.preview };
      if (v.kind === "ask") return { phase: "ask", text: state.text, question: v.question, options: v.options, note: v.note };
      return { phase: "elsewhere", text: state.text, message: v.message, link: v.link, linkLabel: v.linkLabel };
    }
    case "failed":
      return state.phase === "working" || state.phase === "saving" ? { phase: "error", text: state.text, message: event.message } : state;
    case "confirm":
      return state.phase === "preview" ? { phase: "saving", text: state.text, preview: state.preview } : state;
    case "saved": {
      if (state.phase !== "saving") return state;
      const r = event.result;
      if (r.state === "unwired") return { phase: "error", text: state.text, message: UNWIRED_MSG };
      return { phase: "saved", text: "", saved: r.value, at: event.at };
    }
    case "edit":
      return state.phase === "preview" || state.phase === "ask" || state.phase === "elsewhere" ? { phase: "idle", text: state.text } : state;
    case "cancel":
      return state.phase === "working" || state.phase === "saving" ? state : IDLE;
    case "undone":
      return state.phase === "saved" ? { phase: "undone", text: "", title: state.saved.title } : state;
    case "expire":
      return state.phase === "saved" ? IDLE : state;
  }
}

/** The field and Add are disabled while Amara reads or saves. */
export const busy = (s: FlowState) => s.phase === "working" || s.phase === "saving";
