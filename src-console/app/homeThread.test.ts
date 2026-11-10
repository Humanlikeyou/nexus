import { describe, expect, it } from "vitest";
import type { ConversationMessage } from "./conversation";
import { speakingIndex, withVoiceTurns } from "./homeThread";

const typed = (id: string, role: "user" | "assistant", at: string): ConversationMessage => ({ id, role, content: id, timestamp: at });

describe("Home chat thread (design-1110b A2)", () => {
  it("merges spoken turns into the typed thread by time, tagged as voice", () => {
    const msgs = [typed("t1", "user", "2026-11-10T09:00:00+09:00"), typed("t2", "assistant", "2026-11-10T09:00:05+09:00"), typed("t3", "user", "2026-11-10T11:00:00+09:00")];
    const out = withVoiceTurns(msgs, [{ time: "2026-11-10T10:00:00+09:00", text: "what's the weather", route: "hermes", reply: "Clear, 18°." }]);
    expect(out.map((m) => [m.content, m.voice ?? false])).toEqual([
      ["t1", false], ["t2", false], ["What's the weather", true], ["Clear, 18°.", true], ["t3", false],
    ]);
    expect(out[2].role).toBe("user");
    expect(out[3].role).toBe("assistant");
  });

  it("leaves out ignored turns and turns with no text or time", () => {
    const out = withVoiceTurns([], [
      { time: "2026-11-10T10:00:00+09:00", text: "hey", route: "ignored", ignored: "no wake word" },
      { time: "2026-11-10T10:01:00+09:00", text: "", route: "repeat" },
      { text: "no time", route: "hermes", reply: "x" },
    ]);
    expect(out).toEqual([]);
  });

  it("keeps the same array when there are no spoken turns", () => {
    const msgs = [typed("t1", "user", "2026-11-10T09:00:00+09:00")];
    expect(withVoiceTurns(msgs, [])).toBe(msgs);
  });

  it("marks the newest assistant reply only while speaking", () => {
    const msgs = [typed("a", "assistant", "2026-11-10T09:00:00+09:00"), typed("u", "user", "2026-11-10T09:01:00+09:00")];
    expect(speakingIndex(msgs, true)).toBe(0);
    expect(speakingIndex(msgs, false)).toBe(-1);
  });
});
