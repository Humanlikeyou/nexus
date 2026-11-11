import { describe, expect, it } from "vitest";
import { IDLE, busy, flow, type FlowState } from "./flow";

const preview = { title: "Read before bed", fields: [["Target", "20 min · 5 nights a week"]] as Array<[string, string]> };
const run = (events: Parameters<typeof flow>[1][], from: FlowState = IDLE) => events.reduce(flow, from);

describe("design-1111v add flow (state machine)", () => {
  it("idle → working → preview → saving → saved → undo", () => {
    let s = run([{ type: "type", text: "Read 20 minutes before bed" }, { type: "submit" }]);
    expect(s.phase).toBe("working");
    expect(busy(s)).toBe(true);
    // Typing is ignored while Amara reads.
    expect(flow(s, { type: "type", text: "x" })).toBe(s);
    s = flow(s, { type: "parsed", result: { state: "sample", value: { kind: "preview", preview } } });
    expect(s).toMatchObject({ phase: "preview", preview });
    s = flow(s, { type: "confirm" });
    expect(s.phase).toBe("saving");
    s = flow(s, { type: "saved", result: { state: "sample", value: { id: "g9", title: "Read before bed" } }, at: 1000 });
    expect(s).toMatchObject({ phase: "saved", text: "", saved: { id: "g9" }, at: 1000 });
    expect(flow(s, { type: "undone" })).toEqual({ phase: "undone", text: "", title: "Read before bed" });
    expect(flow(s, { type: "expire" })).toEqual(IDLE);
  });

  it("an empty field does nothing", () => {
    expect(run([{ type: "type", text: "   " }, { type: "submit" }]).phase).toBe("idle");
  });

  it("errors keep the text so Try again works", () => {
    let s = run([{ type: "type", text: "Read before bed" }, { type: "submit" }, { type: "failed", message: "Amara isn't reachable." }]);
    expect(s).toEqual({ phase: "error", text: "Read before bed", message: "Amara isn't reachable." });
    s = flow(s, { type: "submit" });
    expect(s).toEqual({ phase: "working", text: "Read before bed" });
  });

  it("unwired reading never pretends to save", () => {
    const s = run([{ type: "type", text: "x" }, { type: "submit" }, { type: "parsed", result: { state: "unwired" } }]);
    expect(s.phase).toBe("error");
    expect(s.text).toBe("x");
  });

  it("asks instead of guessing, then reads the answer", () => {
    let s = run([{ type: "type", text: "Visa expires Sept 14" }, { type: "submit" }, { type: "parsed", result: { state: "sample", value: { kind: "ask", question: "Which year is it?", options: ["2027", "2028"] } } }]);
    expect(s).toMatchObject({ phase: "ask", question: "Which year is it?" });
    s = flow(s, { type: "answer", option: "2027" });
    expect(s).toEqual({ phase: "working", text: "Visa expires Sept 14 (2027)" });
  });

  it("a business date typed in Life points to where it lives", () => {
    const link = { id: "enterprise" as const, view: "ventures/taiyo-skin" };
    const s = run([{ type: "type", text: "Move the launch" }, { type: "submit" }, { type: "parsed", result: { state: "sample", value: { kind: "elsewhere", message: "Lives in Enterprise", link, linkLabel: "Open Taiyo Skin ›" } } }]);
    expect(s).toMatchObject({ phase: "elsewhere", link });
    expect(flow(s, { type: "edit" })).toEqual({ phase: "idle", text: "Move the launch" });
  });

  it("Cancel from a preview clears; Edit keeps the text", () => {
    const s = run([{ type: "type", text: "x" }, { type: "submit" }, { type: "parsed", result: { state: "sample", value: { kind: "preview", preview } } }]);
    expect(flow(s, { type: "cancel" })).toEqual(IDLE);
    expect(flow(s, { type: "edit" })).toEqual({ phase: "idle", text: "x" });
  });
});
