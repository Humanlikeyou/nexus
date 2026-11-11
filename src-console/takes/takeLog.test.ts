// @vitest-environment node
import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: Array<[string, unknown]> = [];
let log: unknown[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: unknown) => {
    calls.push([cmd, args]);
    if (cmd === "take_log_list") return log;
    if (cmd === "take_log_forget") { const id = (args as { id: string }).id; const e = log.find((x) => (x as { id: string }).id === id) as { text: string; forgottenAt: number | null } | undefined; if (!e) return false; e.text = ""; e.forgottenAt = 1; return true; }
    if (cmd === "take_log_append") { log.push((args as { entry: unknown }).entry); return true; }
    return 0;
  }),
}));

import { liveTakes } from "./adapters";
import { entryFromTake, historyView, memoryLineView, resetTakeLogForTest, setTakeLogCache, takeEntryId, withinRetention, type TakeLogEntry } from "./takeLog";
import { runTake, type TakeDeps } from "../amaraTake/generate";
import { validateTake } from "../amaraTake/validate";
import { liveHrTake } from "../hrTake/adapters";
import type { Fact, FactSet } from "../amaraTake/types";

const DAY = 86_400_000;
const NOW = Date.parse("2026-11-11T03:00:00Z"); // 12:00 Tokyo, Wednesday
const e = (id: string, page: TakeLogEntry["page"], ago: number, text: string, over: Partial<TakeLogEntry> = {}): TakeLogEntry => {
  const at = NOW - ago;
  return { id, page, day: new Date(at + 9 * 3_600_000).toISOString().slice(0, 10), generatedAt: at, kind: "daily", text, cited: [], fingerprint: "f", tier: "light", trialId: null, memoryLine: null, forgottenAt: null, ...over };
};

beforeEach(() => { calls.length = 0; log = []; resetTakeLogForTest(); });

describe("take log selectors", () => {
  const entries = [
    e("a", "fitness", 0, "Sleep ran 7.2 h today."),
    e("b", "fitness", DAY, "HRV was 55 yesterday. Nice.", { kind: "followup" }),
    e("c", "neuro", DAY, "Calm 60."),
    e("d", "fitness", 15 * DAY, "Old take."),
    e("gone", "fitness", 2 * DAY, "", { forgottenAt: 5 }),
    e("old", "fitness", 366 * DAY, "Too old."),
  ];
  it("lists one page newest first, hides forgotten and older than 365 days", () => {
    const h = historyView(entries, "fitness", "all", "", NOW);
    expect(h.map((x) => x.id)).toEqual(["a", "b", "d"]);
    expect(h[0]).toMatchObject({ week: "This week", day: "Nov 11", weekday: "Wed" });
    expect(h[2].week).toBe("Week of Oct 26");
    expect(withinRetention(entries, NOW).map((x) => x.id)).not.toContain("old");
  });
  it("filters follow-ups, suggestions (none have a trial yet) and search text", () => {
    expect(historyView(entries, "fitness", "followup", "", NOW).map((x) => x.id)).toEqual(["b"]);
    expect(historyView(entries, "fitness", "suggestion", "", NOW)).toEqual([]);
    expect(historyView(entries, "fitness", "all", "HRV", NOW).map((x) => x.id)).toEqual(["b"]);
  });
  it("builds the memory line from an earlier day's stored take, quoting its first sentence", () => {
    const m = memoryLineView(entries, "fitness", "2026-11-11", NOW);
    expect(m).toEqual({ day: "Nov 10", text: "On Nov 10 I said: “HRV was 55 yesterday.”" });
    expect(memoryLineView([entries[0]], "fitness", "2026-11-11", NOW)).toBeNull();
  });
});

describe("live takes adapter", () => {
  it("is unwired until the log loads, then wires memory, count, history and settings; the rest stay unwired", async () => {
    log = [e("a", "fitness", DAY, "Sleep ran 7.2 h."), e("b", "fitness", 0, "Today.")];
    expect(liveTakes.historyCount("fitness").state).toBe("unwired");
    await vi.waitFor(() => expect(liveTakes.historyCount("fitness")).toEqual({ state: "live", value: 2 }));
    expect(calls[0][0]).toBe("take_log_migrate");
    expect(liveTakes.memory("fitness").state).toBe("live");
    expect(liveTakes.settings()).toEqual({ state: "live", value: { keepDays: 365, summarizeAfter: true, autoDone: true } });
    for (const w of [liveTakes.today("fitness"), liveTakes.outcomes(), liveTakes.week(), liveTakes.goals()]) expect(w.state).toBe("unwired");
    expect((await liveTakes.answer("fitness", "x", "done")).state).toBe("unwired");
    expect(liveTakes.detects("fitness")).toBe(false);
  });
  it("forget blanks one entry and the history drops it", async () => {
    log = [e("a", "fitness", DAY, "keep"), e("b", "fitness", 0, "forget me")];
    setTakeLogCache(log as TakeLogEntry[]);
    expect((await liveTakes.forget("fitness", "b")).state).toBe("live");
    expect(calls.map((c) => c[0])).toContain("take_log_forget");
    const h = liveTakes.history("fitness", "all", "");
    expect(h.state === "live" && h.value.map((x) => x.id)).toEqual(["a"]);
    expect((await liveTakes.forget("fitness", "nope")).state).toBe("unwired");
  });
});

const fact: Fact = { key: "hrv", label: "HRV", value: 55, unit: "ms", display: "55 ms", baseline: { mean: 60, n: 28, display: "60 ms" }, accept: [55] };
const set: FactSet = { page: "fitness", day: "2026-11-11", facts: [fact], fingerprint: "fp1" };

describe("generate appends accepted takes to the log", () => {
  const mk = (reply: string, tier = "light") => {
    const store = new Map<string, string>(); const appended: Array<[string, string]> = [];
    const deps: TakeDeps = { now: () => NOW, kv: { load: async (k) => store.get(k) ?? null, save: async (k, v) => { store.set(k, v); } }, ask: async () => ({ content: reply, tier }), facts: async () => set, log: async (t, tr) => { appended.push([t.text, tr]); } };
    return { deps, appended };
  };
  it("appends one entry per accepted take with the same id the migration uses", async () => {
    const h = mk("HRV is 55 ms today.");
    const r = await runTake("fitness", h.deps);
    expect(r.state).toBe("live");
    expect(h.appended).toEqual([["HRV is 55 ms today.", "light"]]);
    const take = r.state === "live" ? r.value : null;
    expect(take && entryFromTake(take, "light").id).toBe(takeEntryId("fitness", NOW));
  });
  it("appends nothing for a rejected reply (invented number) or a cloud-tier reply", async () => {
    const a = mk("HRV is 55 ms, up 12 from last week."); await runTake("fitness", a.deps); expect(a.appended).toEqual([]);
    const b = mk("HRV is 55 ms.", "claude"); await runTake("fitness", b.deps); expect(b.appended).toEqual([]);
  });
  it("a failing log never blocks the take", async () => {
    const h = mk("HRV is 55 ms."); h.deps.log = async () => { throw new Error("disk"); };
    expect((await runTake("fitness", h.deps)).state).toBe("live");
  });
});

describe("validator", () => {
  it("rejects any number that is not in the facts", () => {
    expect(validateTake("HRV is 55 ms.", [fact]).ok).toBe(true);
    for (const t of ["HRV is 55 ms, 12 higher.", "HRV is 56 ms.", "HRV 55, usual 70."]) expect(validateTake(t, [fact])).toMatchObject({ ok: false, reason: "unsupported_number" });
  });
});

describe("live hr take adapter", () => {
  it("passes backend answers through the shape check and treats errors or odd shapes as unwired", async () => {
    const core = await import("@tauri-apps/api/core");
    const mocked = vi.mocked(core.invoke);
    mocked.mockResolvedValueOnce({ kind: "none_yet", due: true });
    expect(await liveHrTake.status()).toEqual({ state: "live", value: { kind: "none_yet", due: true } });
    mocked.mockResolvedValueOnce({ kind: "bogus" });
    expect((await liveHrTake.refresh("open")).state).toBe("unwired");
    mocked.mockRejectedValueOnce(new Error("x"));
    expect((await liveHrTake.refresh("focus")).state).toBe("unwired");
    mocked.mockResolvedValueOnce({ kind: "none_yet", due: true });
    await liveHrTake.refresh("navigate");
    expect(mocked.mock.calls[mocked.mock.calls.length - 1]).toEqual(["home_hr_take_maybe_refresh", { trigger: "navigate" }]);
  });
});
