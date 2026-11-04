import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ConversationMessage } from "../app/conversation";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot, TerrenceAction } from "../types";
import { ChatField, groupStarts, type ChatFieldProps } from "./ChatScreen";

const loaded: SystemSnapshot = {
  ...INITIAL_SNAPSHOT,
  refreshedAt: Date.parse("2026-10-28T05:00:00.000Z"),
  hermes: { ...INITIAL_SNAPSHOT.hermes, status: "online", model: "hermes-3" },
};

function action(over: Partial<TerrenceAction> = {}): TerrenceAction {
  return {
    id: "a1", toolId: "macos.open_url", title: "Open the dashboard", status: "awaiting_approval", requiresApproval: true,
    parameters: {}, conversationId: null, contextItemIds: [], requestedAt: "2026-10-28T05:00:00Z", updatedAt: "2026-10-28T05:00:00Z",
    approvedAt: null, startedAt: null, completedAt: null, result: null, error: null, retryOfActionId: null, retryAllowed: false,
    summary: "Opens a page", ...over,
  };
}

function msg(id: string, role: "user" | "assistant", content: string, timestamp: string, over: Partial<ConversationMessage> = {}): ConversationMessage {
  return { id, role, content, timestamp, ...over };
}

function setup(over: Partial<ChatFieldProps> = {}) {
  const props: ChatFieldProps = {
    data: loaded, messages: [], sending: false,
    onSend: vi.fn(async () => {}), onNewConversation: vi.fn(), onOpenContext: vi.fn(),
    onApproveAction: vi.fn(), onRetryAction: vi.fn(), onCancelAction: vi.fn(), onRetrySources: vi.fn(),
    ...over,
  };
  render(<ChatField {...props} />);
  return props;
}

describe("ChatField", () => {
  it("agent column: Crews row shows the running count and opens the Crews view", () => {
    const onOpenCrews = vi.fn();
    const runs = [
      { id: "a", name: "a", status: "running", agents: [], started_at: null, task: null, result: null },
      { id: "b", name: "b", status: "completed", agents: [], started_at: null, task: null, result: null },
    ];
    setup({ onOpenCrews, data: { ...loaded, crew: { health: "online", runs, tools: [] } } });
    fireEvent.click(screen.getByRole("button", { name: /Crews/ }));
    expect(screen.getByText("1 running")).toBeInTheDocument();
    expect(onOpenCrews).toHaveBeenCalled();
  });

  it("agent column: Crews row says Not configured without a token", () => {
    setup({ onOpenCrews: vi.fn(), data: { ...loaded, crew: { health: "not_configured", runs: [], tools: [] } } });
    expect(screen.getByText("Not configured")).toBeInTheDocument();
  });

  it("shows a skeleton until the first snapshot lands", () => {
    setup({ data: INITIAL_SNAPSHOT });
    expect(screen.getByRole("status", { name: "Loading" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Amara" })).toBeInTheDocument();
  });

  it("empty state offers priority suggestions that send", async () => {
    const data = { ...loaded, prioritySnapshot: { ...loaded.prioritySnapshot, items: [
      { rank: 1, sourceKind: "context" as const, sourceId: "c1", title: "Ship Amara OS v2", sourceStatus: "active", score: 1, confidence: 1, reasons: [], contextItemIds: [], actionId: null },
    ] } };
    const props = setup({ data });
    expect(screen.getByRole("heading", { name: "Ask Amara anything" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Ship Amara OS v2" }));
    expect(props.onSend).toHaveBeenCalledWith("Ship Amara OS v2", { deep: false });
  });

  it("Enter sends, Shift+Enter adds a newline, and the composer clears", async () => {
    const props = setup();
    const box = screen.getByLabelText("Message Amara");
    await userEvent.type(box, "hello{Shift>}{Enter}{/Shift}there");
    expect(box).toHaveValue("hello\nthere");
    await userEvent.type(box, "{Enter}");
    expect(props.onSend).toHaveBeenCalledWith("hello\nthere", { deep: false });
    expect(box).toHaveValue("");
  });

  it("offline: warning badge, offline placeholder, no assistant badge for an idle backend", () => {
    setup({ data: { ...loaded, hermes: { ...loaded.hermes, status: "offline" } } });
    expect(screen.getByLabelText("Message Amara")).toHaveAttribute("placeholder", "Amara is offline — messages may fail");
    expect(screen.getAllByText("Offline").length).toBeGreaterThan(0);
    expect(screen.queryByText("Not connected")).not.toBeInTheDocument();
  });

  it.each(["not_installed", "offline"] as const)("backend %s (local mode): no assistant row, no OpenClaw label", (status) => {
    setup({ data: { ...loaded, openclaw: { status, online: false, model: "unknown", gateway_url: "ws://127.0.0.1:18789", last_heartbeat: null } } });
    const agents = screen.getByLabelText("Agents and conversations");
    expect(within(agents).queryByText("Assistant")).not.toBeInTheDocument();
    expect(within(agents).queryByText(/OpenClaw|No heartbeat|Not installed/)).not.toBeInTheDocument();
  });

  it("backend installed but silent: 'Assistant backend not reachable' with Retry (warning)", () => {
    const props = setup({ onRetrySources: vi.fn(), data: { ...loaded, openclaw: { status: "configured", online: false, model: "unknown", gateway_url: "ws://127.0.0.1:18789", last_heartbeat: null } } });
    const agents = screen.getByLabelText("Agents and conversations");
    expect(within(agents).getByLabelText("Assistant status: Not reachable")).toBeInTheDocument();
    const block = within(agents).getByRole("status");
    expect(block).toHaveAttribute("data-tone", "warning");
    fireEvent.click(within(block).getByRole("button", { name: "Retry" }));
    expect(props.onRetrySources).toHaveBeenCalled();
    expect(agents.textContent).not.toMatch(/18789|OpenClaw/);
  });

  it("backend reachable: 'Amara' gateway row with its model and an aria-label", () => {
    setup({ data: { ...loaded, openclaw: { status: "online", online: true, model: "minimax-m2.5", gateway_url: "ws://127.0.0.1:18789", last_heartbeat: null } } });
    const agents = screen.getByLabelText("Agents and conversations");
    expect(within(agents).getByText("minimax-m2.5")).toBeInTheDocument();
    expect(within(agents).getAllByLabelText("Amara status: Online").length).toBeGreaterThan(0);
    expect(within(agents).queryByText(/OpenClaw/)).not.toBeInTheDocument();
  });

  it("failed reply shows Retry that resends the previous user message; pending shows Thinking", async () => {
    const messages = [
      msg("u1", "user", "what's next?", "2026-10-28T05:00:00Z"),
      msg("a1", "assistant", "Hermes did not respond", "2026-10-28T05:00:10Z", { status: "failed" }),
    ];
    const props = setup({ messages, sending: false });
    expect(screen.getByText("Failed")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(props.onSend).toHaveBeenCalledWith("what's next?", { deep: false });
  });

  it("shows the thinking bubble while sending", () => {
    setup({ messages: [msg("u1", "user", "hi", "2026-10-28T05:00:00Z")], sending: true });
    expect(screen.getByRole("status", { name: "Amara is thinking" })).toBeInTheDocument();
  });

  it("action cards: approve, cancel, retry; hermes chat actions are hidden", async () => {
    const data = { ...loaded, actions: [
      action(),
      action({ id: "a2", title: "Retry me", status: "failed", retryAllowed: true }),
      action({ id: "a3", toolId: "agent.hermes_chat", title: "Chat turn", status: "succeeded" }),
    ] };
    const props = setup({ data });
    const actions = screen.getByLabelText("Actions");
    expect(within(actions).queryByText("Chat turn")).not.toBeInTheDocument();
    await userEvent.click(within(actions).getByRole("button", { name: "Approve" }));
    await userEvent.click(within(actions).getByRole("button", { name: "Cancel" }));
    await userEvent.click(within(actions).getByRole("button", { name: "Retry" }));
    expect(props.onApproveAction).toHaveBeenCalledWith(expect.objectContaining({ id: "a1" }));
    expect(props.onCancelAction).toHaveBeenCalledWith(expect.objectContaining({ id: "a1" }));
    expect(props.onRetryAction).toHaveBeenCalledWith(expect.objectContaining({ id: "a2" }));
    expect(screen.getByText("Open actions").parentElement).toHaveTextContent("1");
  });

  it("action ledger unavailable shows a retry", async () => {
    const props = setup({ data: { ...loaded, unavailableSources: [{ command: "list_terrence_actions", label: "Actions" }] } });
    expect(screen.getByText("Action ledger unavailable")).toBeInTheDocument();
    await userEvent.click(within(screen.getByLabelText("Actions")).getByRole("button", { name: "Retry" }));
    expect(props.onRetrySources).toHaveBeenCalled();
  });

  it("header buttons and ⌘N / ⌘⌥I shortcuts", async () => {
    const props = setup({ lastSavedAt: Date.now() });
    expect(screen.getByText("Saved just now")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Show context" }));
    await userEvent.keyboard("{Meta>}n{/Meta}");
    await userEvent.keyboard("{Meta>}{Alt>}i{/Alt}{/Meta}");
    expect(props.onNewConversation).toHaveBeenCalledTimes(1);
    expect(props.onOpenContext).toHaveBeenCalledTimes(2);
  });

  it("Amara's subtitle is just 'Hermes Agent'; the model is in the tooltip", () => {
    setup();
    const agents = screen.getByLabelText("Agents and conversations");
    const sub = within(agents).getByText("Hermes Agent");
    expect(sub).toHaveAttribute("title", "hermes-3");
    expect(within(agents).queryByText(/Hermes Agent ·/)).toBeNull();
  });

  it("agent search filters the list", async () => {
    setup();
    await userEvent.type(screen.getByLabelText("Search agents"), "hermes");
    const agents = screen.getByLabelText("Agents and conversations");
    expect(within(agents).getByText("Amara")).toBeInTheDocument();
    await userEvent.clear(screen.getByLabelText("Search agents"));
    await userEvent.type(screen.getByLabelText("Search agents"), "zzz");
    expect(within(agents).queryByText("Amara")).not.toBeInTheDocument();
  });
});

describe("groupStarts", () => {
  it("groups same-sender messages within 2 minutes", () => {
    expect(groupStarts([
      msg("1", "user", "a", "2026-10-28T05:00:00Z"),
      msg("2", "user", "b", "2026-10-28T05:01:00Z"),
      msg("3", "user", "c", "2026-10-28T05:04:00Z"),
      msg("4", "assistant", "d", "2026-10-28T05:04:10Z"),
    ])).toEqual([true, false, true, true]);
  });

  it("'Ask Amara anything' only for a thread with zero items: hidden once an action card exists", () => {
    setup({ data: { ...loaded, actions: [action()] } });
    expect(screen.queryByRole("heading", { name: "Ask Amara anything" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Actions")).toBeInTheDocument();
  });

  it("an action that isn't shown (Hermes chat turn) doesn't count as an item", () => {
    setup({ data: { ...loaded, actions: [action({ id: "h", toolId: "agent.hermes_chat", status: "succeeded" })] } });
    expect(screen.getByRole("heading", { name: "Ask Amara anything" })).toBeInTheDocument();
  });

  it("Deep toggle is off and explained while Hermes is not on the Amara Router", () => {
    setup();
    const deep = screen.getByRole("button", { name: "Deep" });
    expect(deep).toBeDisabled();
    expect(deep).toHaveAttribute("aria-pressed", "false");
    expect(deep.closest(".chat-composer__deep-wrap")).toHaveAttribute("title", "Deep needs the Amara Router");
  });

  it("Deep toggle sends deep: true when Hermes routes through the Amara Router", async () => {
    const props = setup({ data: { ...loaded, hermes: { ...loaded.hermes, routedViaRouter: true } } });
    const deep = screen.getByRole("button", { name: "Deep" });
    await userEvent.click(deep);
    expect(deep).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Message Amara")).toHaveAttribute("placeholder", "Message Amara (Deep)");
    await userEvent.type(screen.getByLabelText("Message Amara"), "plan my week{Enter}");
    expect(props.onSend).toHaveBeenCalledWith("plan my week", { deep: true });
  });

  it("tier badge: Fast/Deep from the router on Amara's replies, nothing when unknown", () => {
    setup({ messages: [
      msg("u1", "user", "hi", "2026-10-28T05:00:00Z"),
      msg("a1", "assistant", "Hello.", "2026-10-28T05:00:05Z", { tier: { tier: "fast", reason: "classifier_easy", source: "router_log" } }),
      msg("u2", "user", "/deep plan", "2026-10-28T05:00:10Z"),
      msg("a2", "assistant", "Here is a plan.", "2026-10-28T05:00:40Z", { tier: { tier: "deep", reason: "slash_deep", source: "router_dry_run" } }),
      msg("a3", "assistant", "No tier known.", "2026-10-28T05:01:00Z"),
    ] });
    const fast = screen.getByLabelText("Answered on the Fast tier");
    expect(fast).toHaveTextContent("Fast");
    expect(fast).toHaveAttribute("title", expect.stringContaining("route log"));
    expect(screen.getByLabelText("Answered on the Deep tier")).toHaveTextContent("Deep");
    expect(within(screen.getByRole("list", { name: "Messages" })).getAllByText(/^(Fast|Deep)$/)).toHaveLength(2);
  });
});
