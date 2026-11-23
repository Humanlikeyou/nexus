import { describe, expect, it } from "vitest";
import { beforeNoonJst, jst, makeSampleDecisions, unwiredDecisions } from "./log";

const draft = { itemId: "f1", subject: "vote" as const, title: "Taiyo Skin · first batch (500 units)", choice: "yes" as const, proposer: "Taiyo Skin crew", sourceLink: { id: "enterprise" as const, view: "ventures/taiyo-skin" }, sourceLabel: "Ventures › Taiyo Skin ›" };

describe("design-1111v decisions log", () => {
  it("appends one record; Undo within 8 s removes it", async () => {
    const log = makeSampleDecisions();
    const before = (log.list() as { value: unknown[] }).value.length;
    const at = new Date("2026-10-28T05:02:00Z");
    const r = await log.append(draft, at);
    expect(r.state).toBe("sample");
    expect((log.list() as { value: unknown[] }).value).toHaveLength(before + 1);
    const id = (r as { value: { id: string } }).value.id;
    expect(await log.undo(id, new Date(at.getTime() + 7_000))).toEqual({ state: "sample", value: { ok: true } });
    expect((log.list() as { value: unknown[] }).value).toHaveLength(before);
  });

  it("after 8 s a record stays (a correction is a new record)", async () => {
    const log = makeSampleDecisions();
    const at = new Date("2026-10-28T05:02:00Z");
    const r = await log.append(draft, at);
    const id = (r as { value: { id: string } }).value.id;
    expect(await log.undo(id, new Date(at.getTime() + 9_000))).toEqual({ state: "sample", value: { ok: false } });
  });

  it("an outcome is added later without changing the choice", async () => {
    const log = makeSampleDecisions();
    const r = await log.append(draft);
    const id = (r as { value: { id: string } }).value.id;
    const o = await log.addOutcome(id, "Batch ordered");
    expect(o).toMatchObject({ value: { choice: "yes", outcome: "Batch ordered" } });
  });

  it("unwired records nothing", async () => {
    expect(unwiredDecisions.list()).toEqual({ state: "unwired" });
    expect(await unwiredDecisions.append(draft)).toEqual({ state: "unwired" });
  });

  it("times read in JST; before noon is JST noon", () => {
    expect(jst("2026-10-28T05:02:00Z")).toBe("Oct 28 · 14:02");
    expect(beforeNoonJst("2026-10-28T02:59:00Z")).toBe(true);
    expect(beforeNoonJst("2026-10-28T03:00:00Z")).toBe(false);
  });
});
