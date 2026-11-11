import { describe, expect, it } from "vitest";
import { makeLiveAdd } from "../adds/adapters";
import { IDLE, flow } from "../adds/flow";
import { makeLiveDecisions, type DecisionRecord } from "../decisions/log";
import { mergeInbox, type InboxPointer, type SourceState } from "../inbox/needsYou";
import { checkInFrom, lifeAdminFrom, makeLivePersonal } from "../personal/adapters";
import type { Invoke } from "./localStores";

/** A fake backend: records every command name and answers from a table. */
function backend(answers: Record<string, (args?: Record<string, unknown>) => unknown>) {
  const calls: string[] = [];
  const call: Invoke = async <T,>(command: string, args?: Record<string, unknown>) => {
    calls.push(command);
    const f = answers[command];
    if (!f) throw new Error(`not mocked: ${command}`);
    return f(args) as T;
  };
  return { call, calls };
}

const draft = { itemId: "f1", subject: "vote" as const, title: "Taiyo Skin · quote", choice: "yes" as const, proposer: "Taiyo Skin crew", sourceLink: { id: "enterprise" as const, view: "ventures/taiyo-skin" }, sourceLabel: "Ventures › Taiyo Skin ›" };
const rec = (id: string): DecisionRecord => ({ ...draft, id, decidedAt: "2026-11-11T10:00:00.000+09:00" });

describe("live decisions log", () => {
  it("is unwired until the first read works, then lists live records", async () => {
    const { call } = backend({ decisions_list: () => [rec("d1")] });
    const log = makeLiveDecisions(call);
    expect(log.wired).toBe(false);
    expect(log.list()).toEqual({ state: "unwired" });
    await log.refresh();
    expect(log.wired).toBe(true);
    expect(log.list()).toEqual({ state: "live", value: [rec("d1")] });
    const down = makeLiveDecisions(backend({}).call);
    await down.refresh();
    expect(down.wired).toBe(false);
  });

  it("Record vote writes one record, Undo removes it, and no ledger or money command is ever invoked", async () => {
    const store: DecisionRecord[] = [];
    const { call, calls } = backend({
      decisions_list: () => [...store],
      decisions_append: () => { const r = rec("d9"); store.unshift(r); return r; },
      decisions_undo: () => { store.shift(); return true; },
      decisions_add_outcome: () => ({ ...rec("d9"), outcome: "Ordered" }),
    });
    const log = makeLiveDecisions(call);
    await log.refresh();
    const r = await log.append(draft);
    expect(r).toMatchObject({ state: "live", value: { id: "d9" } });
    expect((log.list() as { value: unknown[] }).value).toHaveLength(1);
    expect(await log.undo("d9")).toEqual({ state: "live", value: { ok: true } });
    expect((log.list() as { value: unknown[] }).value).toHaveLength(0);
    await log.addOutcome("d9", "Ordered");
    expect(calls.length).toBeGreaterThan(3);
    expect(calls.every((c) => c.startsWith("decisions_"))).toBe(true);
    expect(calls.some((c) => /ledger|arm_|fund|pay|send/.test(c))).toBe(false);
  });

  it("a failed write resolves unwired and keeps the list as it was", async () => {
    const { call } = backend({ decisions_list: () => [], decisions_append: () => { throw new Error("full"); } });
    const log = makeLiveDecisions(call);
    await log.refresh();
    expect(await log.append(draft)).toEqual({ state: "unwired" });
    expect((log.list() as { value: unknown[] }).value).toHaveLength(0);
  });

  it("an undo the backend refuses (past 8 s) keeps the record", async () => {
    const { call } = backend({ decisions_list: () => [rec("d1")], decisions_undo: () => false });
    const log = makeLiveDecisions(call);
    await log.refresh();
    expect(await log.undo("d1")).toEqual({ state: "live", value: { ok: false } });
    expect((log.list() as { value: unknown[] }).value).toHaveLength(1);
  });
});

describe("live add adapter: one write path, idle → working → preview → saved/undo → error", () => {
  const preview = { title: "Read before bed", fields: [["Target", "20 min · 5 nights a week"]] as Array<[string, string]>, payload: { kind: "goal", title: "Read before bed" } };
  it("wires after the History read and walks the whole flow through the same commands", async () => {
    const hist: Array<{ id: string; when: string; text: string; via: string }> = [];
    const { call, calls } = backend({
      add_history: () => [...hist],
      add_preview: () => ({ kind: "preview", preview }),
      add_save: (a) => { hist.unshift({ id: "g-1", when: "Nov 12", text: "Added Read before bed", via: String(a?.via) }); return { id: "g-1", title: "Read before bed" }; },
      add_undo: () => { hist.shift(); return true; },
    });
    const add = makeLiveAdd("goal", call);
    expect(add.wired).toBe(false);
    await add.probe();
    expect(add.wired).toBe(true);

    let s = flow(flow(IDLE, { type: "type", text: "Read 20 minutes before bed, 5 nights a week" }), { type: "submit" });
    expect(s.phase).toBe("working");
    s = flow(s, { type: "parsed", result: await add.parse(s.text) });
    expect(s).toMatchObject({ phase: "preview", preview: { payload: { kind: "goal" } } });
    s = flow(s, { type: "confirm" });
    if (s.phase !== "saving") throw new Error("expected saving");
    s = flow(s, { type: "saved", result: await add.save(s.preview, "voice"), at: 1 });
    expect(s).toMatchObject({ phase: "saved", saved: { id: "g-1" } });
    expect(add.history()).toMatchObject({ state: "live", value: [{ via: "voice" }] });
    await add.undo("g-1");
    s = flow(s, { type: "undone" });
    expect(s.phase).toBe("undone");
    expect((add.history() as { value: unknown[] }).value).toHaveLength(0);
    expect(new Set(calls)).toEqual(new Set(["add_history", "add_preview", "add_save", "add_undo"]));
  });

  it("a backend error lands in the error phase with the text kept; a missing backend stays unwired", async () => {
    const down = makeLiveAdd("task", backend({}).call);
    await down.probe();
    expect(down.wired).toBe(false);
    expect(down.history()).toEqual({ state: "unwired" });
    let s = flow(flow(IDLE, { type: "type", text: "Order jars" }), { type: "submit" });
    s = await down.parse(s.text).then((result) => flow(s, { type: "parsed", result }), () => flow(s, { type: "failed", message: "Couldn't read it" }));
    expect(s).toEqual({ phase: "error", text: "Order jars", message: "Couldn't read it" });
  });

  it("an undo past 8 s rejects so the UI never shows 'Undone' for a record that stayed", async () => {
    const add = makeLiveAdd("date", backend({ add_history: () => [], add_undo: () => false }).call);
    await expect(add.undo("x")).rejects.toThrow(/8 second/);
  });
});

describe("live personal store (life dates, check-ins)", () => {
  const row = (id: string, inDays: number, win: number | null) => ({ id, title: `Date ${id}`, date: "2026-12-18", lane: "personal" as const, sub: "", inDays, activeWindow: win,
    reminders: [{ days: 90, on: "2026-09-19", passed: true }, { days: 30, on: "2026-11-18", passed: inDays < 30 }, { days: 7, on: "2026-12-11", passed: inDays < 7 }] });
  it("maps stored dates to the Dates page, with the next reminder rung and the open window", () => {
    const a = lifeAdminFrom([row("a", 3, 7), row("b", 40, null), row("old", -2, null)]);
    expect(a.dates.map((d) => d.id)).toEqual(["a", "b"]);
    expect(a.dates[0]).toMatchObject({ due: true, date: "Dec 18", reminder: "7 days · Dec 11", inDays: 3, lane: "personal" });
    expect(a.dates[1].reminder).toBe("30 days · Nov 18");
    expect(a.dates[1].due).toBe(false);
    expect(a.next).toBe("Date a in 3 days");
  });
  it("check-ins list newest first as live data; saving needs mood and energy; no audio field is sent", async () => {
    const sent: unknown[] = [];
    const wire = { id: "c1", at: "2026-11-11T22:05:00.000+09:00", day: "2026-11-11", mood: 4, energy: 3, tags: ["training"], text: "ok", transcribed: false };
    const { call } = backend({ life_date_list: () => [], checkin_list: () => [wire], checkin_save: (a) => { sent.push(a); return wire; } });
    const p = makeLivePersonal(call);
    expect(p.checkins()).toEqual({ state: "unwired" });
    await p.refresh();
    expect(p.checkins()).toMatchObject({ state: "live", value: [{ id: "c1", mood: 4 }] });
    expect(p.lifeAdmin()).toMatchObject({ state: "live" });
    expect(await p.saveCheckIn({ mood: null, energy: 3, tags: [], text: "" })).toEqual({ state: "unwired" });
    expect(await p.saveCheckIn({ mood: 4, energy: 3, tags: ["training"], text: "ok" })).toMatchObject({ state: "live" });
    expect(Object.keys((sent[0] as { draft: object }).draft).sort()).toEqual(["energy", "mood", "tags", "text"]);
    expect(checkInFrom(wire).when).toMatch(/Wed/);
  });
});

describe("Needs you today from the aggregator: rows only link", () => {
  it("merged rows carry pointer fields only and no handler", () => {
    const rows: InboxPointer[] = [
      { source: "decisions", id: "vote-f1", title: "t", due: null, reason: "r", deepLink: { id: "enterprise", view: "decisions" } },
      { source: "crew", id: "crew-r1", title: "c", due: null, reason: "r", deepLink: { id: "chat", view: "crews" } },
      { source: "checkin", id: "checkin-today", title: "Evening check-in", due: null, reason: "r", deepLink: { id: "terrance", view: "checkin" }, quiet: true },
    ];
    const states: SourceState[] = [{ source: "decisions", state: "ok", rows: [rows[0]] }, { source: "crew", state: "ok", rows: [rows[1]] }, { source: "checkin", state: "ok", rows: [rows[2]] }, { source: "dates", state: "failed", rows: [] }];
    const inbox = mergeInbox(states);
    expect(inbox.count).toBe(2);
    expect(inbox.failed).toEqual(["dates"]);
    for (const r of inbox.rows) {
      expect(Object.keys(r).every((k) => ["source", "id", "title", "due", "reason", "deepLink", "badge", "quiet"].includes(k))).toBe(true);
      expect(Object.values(r).some((v) => typeof v === "function")).toBe(false);
    }
  });
});
