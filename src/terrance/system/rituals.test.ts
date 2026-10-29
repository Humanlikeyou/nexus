import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const m = vi.hoisted(() => ({
  kv: new Map<string, string>(),
  sends: [] as string[],
  refuse: null as string | null,
  bus: { register: vi.fn((name: string, _type?: string) => name), complete: vi.fn(), fail: vi.fn(), suppress: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: Record<string, string>) => {
    if (cmd === "load_kv") return m.kv.get(args.key) ?? null;
    if (cmd === "save_kv") { m.kv.set(args.key, args.value); return; }
    if (cmd === "send_telegram_message") { if (m.refuse) throw m.refuse; m.sends.push(args.text); return; }
    throw new Error(`unexpected ${cmd}`);
  }),
}));
vi.mock("./activityBus", () => ({ activityBus: m.bus }));

import { listRituals, planRituals, type RitualState } from "./rituals";

/** Fresh module per test: ritual state and send backoff live in module scope. */
async function tick() {
  return (await import("./rituals")).tickRituals();
}

// Thursday 2026-10-29, local time (the box and the Mac are both Asia/Tokyo; the plan uses local hours).
const at = (h: number, min: number, day = 29) => new Date(2026, 9, day, h, min);
const morning = ["presence-drill", "command-phrase", "alpha-breathing"];
const empty = (): RitualState => ({ lastRunTimestamps: {} });

describe("planRituals", () => {
  it("2.5h-late launch: all three 08:00 rituals are skipped, nothing fires", () => {
    const plan = planRituals(listRituals(), empty(), at(10, 39));
    expect(plan.fire).toBeNull();
    expect(plan.skip.map((r) => r.id)).toEqual(morning);
  });

  it("fires only within 30 min of the scheduled time, one per tick, earliest first", () => {
    expect(planRituals(listRituals(), empty(), at(7, 59)).fire).toBeNull();
    const p = planRituals(listRituals(), empty(), at(8, 12));
    expect(p.fire?.id).toBe("presence-drill");
    expect(p.skip).toEqual([]);
    expect(planRituals(listRituals(), empty(), at(8, 30)).fire?.id).toBe("presence-drill");
    const late = planRituals(listRituals(), empty(), at(8, 31));
    expect(late.skip.map((r) => r.id)).toEqual(["presence-drill"]);
    expect(late.fire?.id).toBe("command-phrase");
  });

  it("does not repeat a ritual sent or skipped today, but does on the next day", () => {
    const st: RitualState = { lastRunTimestamps: { "presence-drill": at(8, 0).getTime() }, skippedDays: { "command-phrase": "2026-10-29" } };
    expect(planRituals(listRituals(), st, at(8, 15)).fire?.id).toBe("alpha-breathing");
    expect(planRituals(listRituals(), st, at(8, 1, 30)).fire?.id).toBe("presence-drill");
  });

  it("weekly and monthly rituals only count on their day; backed-off rituals wait", () => {
    // Monday 2026-10-26 08:30 Weekly Target Lock; Thursday has none.
    expect(planRituals(listRituals(), empty(), at(8, 35, 26)).skip.map((r) => r.id)).toContain("presence-drill");
    expect(planRituals(listRituals(), { lastRunTimestamps: Object.fromEntries(morning.map((id) => [id, at(8, 0, 26).getTime()])) }, at(8, 35, 26)).fire?.id).toBe("weekly-target-lock");
    expect(planRituals(listRituals(), empty(), at(8, 35)).fire?.id).toBe("command-phrase"); // 08:05 + 30 min is still in
    expect(planRituals(listRituals(), empty(), at(8, 41)).fire).toBeNull();
    expect(planRituals(listRituals(), empty(), at(8, 2), (id) => id === "presence-drill").fire).toBeNull();
  });
});

describe("tickRituals", () => {
  beforeEach(() => { m.kv.clear(); m.sends.length = 0; vi.clearAllMocks(); vi.resetModules(); vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("late launch sends nothing, records the skips in state and Activity, and stays quiet next tick", async () => {
    vi.setSystemTime(at(10, 39));
    await tick();
    expect(m.sends).toEqual([]);
    const st = JSON.parse(m.kv.get("ritual_timestamps") ?? "{}") as RitualState;
    expect(Object.keys(st.skippedDays ?? {}).sort()).toEqual([...morning].sort());
    expect(m.bus.complete.mock.calls.map((c) => c[1])).toEqual(Array(3).fill("Skipped (missed window)"));
    m.bus.complete.mockClear();
    vi.setSystemTime(at(10, 44));
    await tick();
    expect(m.sends).toEqual([]);
    expect(m.bus.register).toHaveBeenCalledTimes(3);
  });

  it("on time: one send per tick", async () => {
    vi.setSystemTime(at(8, 10));
    await tick();
    expect(m.sends).toHaveLength(1);
    expect(m.sends[0]).toContain("Presence Drill");
    vi.setSystemTime(at(8, 15));
    await tick();
    expect(m.sends).toHaveLength(2);
    expect(m.sends[1]).toContain("Set your intent");
  });
});

describe("kill switch (lead decision 2026-10-29)", () => {
  beforeEach(() => { vi.resetModules(); vi.useFakeTimers(); m.kv.clear(); m.sends = []; vi.clearAllMocks(); });
  afterEach(() => { m.refuse = null; vi.useRealTimers(); });

  it("a suppressed ritual send is 'Suppressed' (neutral), not an error, no backoff, once per day", async () => {
    m.refuse = "Outbound sends are disabled for this session (NEXUS_OUTBOUND_DISABLED).";
    vi.setSystemTime(at(8, 2));
    await tick();
    expect(m.bus.suppress).toHaveBeenCalledTimes(1);
    expect(m.bus.suppress).toHaveBeenCalledWith("Ritual: Presence Drill", "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)");
    expect(m.bus.fail).not.toHaveBeenCalled();
    vi.setSystemTime(at(8, 7));
    await tick();
    // presence-drill is done for today; the next ritual (not backed off) is the one handled now.
    expect(m.bus.suppress.mock.calls.filter((c) => c[0] === "Ritual: Presence Drill")).toHaveLength(1);
    expect(m.bus.fail).not.toHaveBeenCalled();
  });

  it("rituals register with the 'ritual' activity type, not 'daily_summary'", async () => {
    vi.setSystemTime(at(10, 39));
    await tick();
    expect(m.bus.register.mock.calls.length).toBeGreaterThan(0);
    for (const c of m.bus.register.mock.calls) expect(c[1]).toBe("ritual");
  });

  it("a real delivery failure is still an error with a safe reason", async () => {
    m.refuse = "POST https://api.telegram.org/botXYZ failed";
    vi.setSystemTime(at(8, 2));
    await tick();
    expect(m.bus.fail).toHaveBeenCalledWith("Ritual: Presence Drill", "delivery failed");
    expect(m.bus.suppress).not.toHaveBeenCalled();
  });
});
