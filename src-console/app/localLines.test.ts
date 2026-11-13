import { describe, expect, it } from "vitest";
import { toSessionEntries } from "../hooks/useIdleAutoSave";
import { historyTurns } from "./amaraChat";
import type { ConversationMessage } from "./conversation";

const msgs: ConversationMessage[] = [
  { id: "1", role: "user", content: "hello", timestamp: "2026-11-13T09:00:00Z" },
  { id: "2", role: "user", content: "add task order jars", timestamp: "2026-11-13T09:00:01Z", local: true },
  { id: "3", role: "assistant", content: "Check before saving (task): Order jars. Say \"confirm\"", timestamp: "2026-11-13T09:00:01Z", local: true },
  { id: "4", role: "assistant", content: "hi", timestamp: "2026-11-13T09:00:02Z" },
];

describe("L1: local add lines stay out of Hermes and the session summary", () => {
  it("historyTurns drops them", () => { expect(historyTurns(msgs).map((t) => t.content)).toEqual(["hello", "hi"]); });
  it("toSessionEntries drops them", () => { expect(toSessionEntries(msgs).map((e) => e.text)).toEqual(["hello", "hi"]); });
});
