import { describe, expect, it } from "vitest";
import type { FollowupItem } from "../types";
import { FOLLOWUP_ERROR_COPY, changedBySave, dayLabel, dueState, followupErrorKind, itemPatch, nextError, textError, todayIso, topicError } from "./followups";

const item = (over: Partial<FollowupItem> = {}): FollowupItem => ({ id: 1, text: "Call", added: null, due: null, next: null, notes: [], lastNudged: null, done: null, ...over });

describe("followups", () => {
  it.each([
    ["malformed_file", "malformed_file"], [{ kind: "not_found" }, "not_found"], [new Error("storage_error"), "storage_error"],
    ["Command followups_list not found", "backend_missing"], [new Error("e2e mock: followups_list not mocked"), "backend_missing"], ["boom", "unknown"], [null, "unknown"],
  ])("classifies %j as %s", (error, kind) => { expect(followupErrorKind(error)).toBe(kind); });

  it("never puts a technical code in the copy", () => {
    for (const copy of Object.values(FOLLOWUP_ERROR_COPY)) expect(copy).not.toMatch(/[a-z]+_[a-z]+/);
    expect(FOLLOWUP_ERROR_COPY.malformed_file).toBe("Amara's list couldn't be read, so nothing was changed.");
  });

  it("reads list dates as local days and passes other text through", () => {
    expect(dayLabel("2026-11-05")).toBe("Nov 5");
    expect(dayLabel("next week")).toBe("next week");
    expect(todayIso(new Date(2026, 0, 3))).toBe("2026-01-03");
  });

  it("works out due state", () => {
    expect(dueState("2026-10-27", "2026-10-28")).toBe("overdue");
    expect(dueState("2026-10-28", "2026-10-28")).toBe("today");
    expect(dueState("2026-10-29", "2026-10-28")).toBe("later");
    expect(dueState(null, "2026-10-28")).toBe("none");
    expect(dueState("soon", "2026-10-28")).toBe("none");
  });

  it("patches only what changed, and clears with null", () => {
    const base = item({ due: "2026-10-28", next: "Ring" });
    expect(itemPatch(base, { text: "Call", due: "2026-10-28", next: "Ring" })).toEqual({});
    expect(itemPatch(base, { text: " Call back ", due: "2026-10-28", next: "Ring" })).toEqual({ text: "Call back" });
    expect(itemPatch(base, { text: "Call", due: "", next: "  " })).toEqual({ due: null, next: null });
  });

  it("spots text the backend changed, looking only at what was sent", () => {
    const saved = item({ text: "Call", next: "Ring" });
    expect(changedBySave({ text: "Call", next: "Ring" }, saved)).toBe(false);
    expect(changedBySave({ text: "Call\u200B" }, saved)).toBe(true);
    expect(changedBySave({ next: "Ring now" }, saved)).toBe(true);
    expect(changedBySave({ next: null }, item())).toBe(false);
    expect(changedBySave({}, saved)).toBe(false);
  });

  it("matches the backend's limits", () => {
    expect(textError("  ")).toBeTruthy();
    expect(textError("a".repeat(500))).toBeUndefined();
    expect(textError("a".repeat(501))).toBeTruthy();
    expect(nextError("")).toBeUndefined();
    expect(topicError("ab")).toBeTruthy();
    expect(topicError("abc")).toBeUndefined();
    expect(topicError("a".repeat(201))).toBeTruthy();
  });
});
