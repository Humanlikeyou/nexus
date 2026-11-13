import { amaraTurn, amaraTurnCancel } from "../bridge";
import type { TurnEvent } from "../types";

/** localStorage flag for the Rust turn pipeline (`amara_turn`). Off unless exactly "1". */
export const TURN_PIPELINE_KEY = "nexus.turnPipeline";

export function turnPipelineEnabled(): boolean {
  try { return window.localStorage.getItem(TURN_PIPELINE_KEY) === "1"; } catch { return false; }
}

/** Fixed copy when the turn could not even start (the command refused or the call failed). */
export const TURN_START_FAILED = "Amara couldn't start that turn.";

const GENERIC_TURN_ERROR = "Amara couldn't answer right now.";
/** Fixed copy per Rust error code. The event's own message is never shown. */
const TURN_ERROR_COPY: Record<string, string> = {
  local_only: "Local only right now. I can't do that without reaching outside this Mac, so I haven't tried. Ask me something I can answer from here, or say \"Amara, back to normal.\"",
  invalid_input: "That message can't be sent as it is.",
  invalid_conversation: "That message can't be sent as it is.",
  start_failed: TURN_START_FAILED,
  busy: TURN_START_FAILED,
  not_loopback: "Amara couldn't answer that locally.",
  claude_refused: "Amara couldn't answer that locally.",
  bad_tier: "Amara couldn't answer that locally.",
  timeout: "Amara took too long to answer.",
  stalled: "Amara took too long to answer.",
  unreachable: "Amara isn't responding right now. Check that the Amara router is running.",
  empty: "Amara didn't have an answer for that.",
  persist_failed: "Amara couldn't save that reply.",
  internal: "Amara hit a problem with that turn.",
};
export const turnErrorCopy = (code: string): string => Object.prototype.hasOwnProperty.call(TURN_ERROR_COPY, code) ? TURN_ERROR_COPY[code] : GENERIC_TURN_ERROR;

export type TurnDone = Extract<TurnEvent, { type: "done" }>;

export interface TurnHandlers {
  onStatus?: (phase: string, tier?: "fast" | "deep" | "claude") => void;
  /** `soFar` is every delta joined. Deltas may be few and large or many and tiny; `done.text` is authoritative. */
  onDelta?: (text: string, soFar: string) => void;
  onAction?: (kind: string, payload: unknown) => void;
  onDone: (done: TurnDone, streamed: string) => void;
  onError: (code: string, message: string) => void;
}

export interface RunningTurn {
  /** Resolves after the single terminal event (done or error), or after cancel(). Never rejects. */
  finished: Promise<void>;
  /** Stop: ends the turn locally at once and asks Rust to cancel (even if the turn id has not arrived yet). */
  cancel: () => void;
}

/**
 * Starts one `amara_turn` and maps its channel events to handlers. Text is only ever kept as plain strings.
 * Unknown event types are ignored; events after the terminal one are ignored.
 */
export function runTurn(
  input: { conversationId: string; text: string; deep?: boolean; haNames?: string[] },
  surface: "typed" | "voice" | "lens" | "window",
  handlers: TurnHandlers,
): RunningTurn {
  let settle!: () => void;
  const finished = new Promise<void>((resolve) => { settle = resolve; });
  let ended = false;
  let turnId: string | null = null;
  let cancelRequested = false;
  let streamed = "";
  const end = () => { ended = true; settle(); };

  const onEvent = (event: TurnEvent) => {
    if (ended || !event || typeof event !== "object") return;
    switch (event.type) {
      case "status":
        handlers.onStatus?.(event.phase, event.tier);
        break;
      case "delta":
        if (typeof event.text === "string" && event.text !== "") {
          streamed += event.text;
          handlers.onDelta?.(event.text, streamed);
        }
        break;
      case "action":
        handlers.onAction?.(event.kind, event.payload);
        break;
      case "done":
        end();
        handlers.onDone(event, streamed);
        break;
      case "error":
        end();
        handlers.onError(String(event.code), turnErrorCopy(String(event.code)));
        break;
      default:
        break;
    }
  };

  amaraTurn(input, surface, onEvent).then((id) => {
    turnId = id;
    if (cancelRequested) void amaraTurnCancel(id).catch(() => undefined);
  }, () => {
    if (ended) return;
    end();
    handlers.onError("start_failed", TURN_START_FAILED);
  });

  return {
    finished,
    cancel: () => {
      if (ended) return;
      cancelRequested = true;
      end();
      if (turnId !== null) void amaraTurnCancel(turnId).catch(() => undefined);
    },
  };
}

/** Everything `sendHome` needs, injected so the routing order is testable. */
export interface SendHomeDeps {
  handleChatAdd: (message: string) => Promise<boolean>;
  handleNavyPrompt: (message: string, openedBy: "chat") => boolean;
  handleWindowPrompt: (message: string, openedBy: "chat") => boolean;
  /** Stateful window commands (close, focus, expand, undo) read live window state, so they stay in the renderer. */
  resolveWindowIntent: (message: string) => { type: string } | null;
  applyWindowIntent: (intent: never) => void;
  enterNavy: (openedBy: "chat") => void;
  exitNavy: (openedBy: "chat") => void;
  sendToTerrence: (message: string, options?: { deep?: boolean; turnPipeline?: boolean; onAction?: (kind: string, payload: unknown) => void }) => Promise<void>;
  enabled?: () => boolean;
}

/**
 * The Home chat send. Flag off: the old renderer pre-route (adds, navy, windows) then the old send, unchanged.
 * Flag on: Rust owns navy, window open and Home Assistant, so none of them run here before the call (adds still run here, see below);
 * only the stateful window commands stay. Rust's `navy` and `window_open` actions drive the same renderer effects.
 */
export function createSendHome(deps: SendHomeDeps) {
  return async (message: string, options?: { deep?: boolean }): Promise<void> => {
    // The one flag read for this send; the hook uses what we pass.
    const turnPipeline = (deps.enabled ?? turnPipelineEnabled)();
    if (!turnPipeline) {
      if (await deps.handleChatAdd(message)) return;
      if (deps.handleNavyPrompt(message, "chat")) return;
      if (deps.handleWindowPrompt(message, "chat")) return;
      await deps.sendToTerrence(message, { ...options, turnPipeline: false });
      return;
    }
    // Adds stay in the renderer until Rust has an adds port (its hook is a stub): a handled add never reaches amara_turn.
    if (await deps.handleChatAdd(message)) return;
    const intent = deps.resolveWindowIntent(message);
    if (intent && intent.type !== "open") {
      deps.applyWindowIntent(intent as never);
      return;
    }
    await deps.sendToTerrence(message, {
      ...options,
      turnPipeline: true,
      onAction: (kind, payload) => {
        if (kind === "navy") {
          if ((payload as { on?: unknown } | null)?.on === false) deps.exitNavy("chat");
          else deps.enterNavy("chat");
        } else if (kind === "window_open") {
          // Rust decided this is a window request; the renderer only builds the animation from the catalog.
          deps.handleWindowPrompt(message, "chat");
        }
      },
    });
  };
}
