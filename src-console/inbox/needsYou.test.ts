import { describe, expect, it } from "vitest";
import { approvalPointers, checkinPointer, mergeInbox, type InboxPointer, type SourceState } from "./needsYou";
import type { TerrenceAction } from "../types";

const row = (id: string, source: InboxPointer["source"], extra: Partial<InboxPointer> = {}) => ({ id, source, title: id, due: null, reason: "", deepLink: { id: "terrance", view: "main" }, ...extra }) as InboxPointer;

describe("Needs you today: one inbox of pointers (design-1111v §3)", () => {
  it("merges every source into one list; quiet rows show but don't count", () => {
    const sources: SourceState[] = [
      { source: "todos", state: "ok", rows: [row("t1", "todos")] },
      { source: "approvals", state: "ok", rows: [row("a1", "approvals")] },
      { source: "checkin", state: "ok", rows: checkinPointer(false, "8:00 PM") },
      { source: "dates", state: "failed", rows: [] },
      { source: "crew", state: "unwired", rows: [] },
    ];
    const inbox = mergeInbox(sources);
    expect(inbox.rows.map((r) => r.source)).toEqual(expect.arrayContaining(["todos", "approvals", "checkin"]));
    expect(inbox.count).toBe(2);
    expect(inbox.failed).toEqual(["dates"]);
    expect(inbox.unwired).toEqual(["crew"]);
    expect(inbox.loading).toBe(false);
  });
  it("is loading until every source settles", () => {
    expect(mergeInbox([{ source: "todos", state: "loading", rows: [] }]).loading).toBe(true);
  });
  it("the check-in row is quiet, and gone once done today", () => {
    expect(checkinPointer(false, "8:00 PM")[0].quiet).toBe(true);
    expect(checkinPointer(true, "8:00 PM")).toEqual([]);
  });
  it("only actions awaiting approval become pointers", () => {
    const a = (id: string, status: string) => ({ id, toolId: "x", title: id, status }) as unknown as TerrenceAction;
    expect(approvalPointers([a("1", "awaiting_approval"), a("2", "succeeded")]).map((p) => p.source)).toEqual(["approvals"]);
  });
});
