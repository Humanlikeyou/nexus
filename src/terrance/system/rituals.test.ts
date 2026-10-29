import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const m = vi.hoisted(() => ({
  kv: new Map<string, string>(),
  sends: [] as string[],
  refuse: null as string | null,
  bus: { register: vi.fn((name: string, _type?: string) => name), complete: vi.fn(), fail: vi.fn(), suppress: vi.fn(), skip: vi.fn() },
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

import { listRituals, planRituals, type Ritual, type RitualState } from "./rituals";

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
    expect(m.bus.skip.mock.calls.map((c) => c[1])).toEqual(Array(3).fill("Skipped (missed window)"));
    expect(m.bus.complete).not.toHaveBeenCalled();
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

/** Every Activity outcome recorded for one ritual, in order: [method, detail]. */
function outcomes(name: string): [string, string][] {
  const calls: [string, unknown[]][] = [];
  for (const k of ["complete", "fail", "suppress", "skip"] as const) {
    for (const [i, c] of m.bus[k].mock.calls.entries()) calls.push([k, [m.bus[k].mock.invocationCallOrder[i], ...c]]);
  }
  return calls
    .filter(([, c]) => c[1] === `Ritual: ${name}`)
    .sort((a, b) => (a[1][0] as number) - (b[1][0] as number))
    .map(([k, c]) => [k, String(c[2])]);
}

describe("window edges (inclusive: a ritual still fires at exactly slot + 30:00)", () => {
  beforeEach(() => { m.kv.clear(); m.sends = []; vi.clearAllMocks(); vi.resetModules(); vi.useFakeTimers(); });
  afterEach(() => { m.refuse = null; vi.useRealTimers(); });

  // Social Expansion at 15:00: the 08:00–12:30 rituals are long past and get skipped, which is fine.
  const social = (min: number, sec: number) => new Date(2026, 9, 29, 15, min, sec);

  it("+29:59 fires", async () => {
    vi.setSystemTime(social(29, 59));
    await tick();
    expect(m.sends.some((t) => t.includes("Social"))).toBe(true);
    expect(outcomes("Social Expansion")).toEqual([["complete", "Sent: Social Expansion"]]);
  });

  it("+30:00 fires (planRituals skips only when now − slot > RITUAL_WINDOW_MS)", async () => {
    vi.setSystemTime(social(30, 0));
    await tick();
    expect(outcomes("Social Expansion")).toEqual([["complete", "Sent: Social Expansion"]]);
  });

  it("+30:01 is skipped once, then quiet", async () => {
    vi.setSystemTime(social(30, 1));
    await tick();
    vi.setSystemTime(social(35, 1));
    await tick();
    expect(m.sends).toEqual([]);
    expect(outcomes("Social Expansion")).toEqual([["skip", "Skipped (missed window)"]]);
  });
});

describe("local midnight (Asia/Tokyo)", () => {
  beforeEach(() => { m.kv.clear(); m.sends = []; vi.clearAllMocks(); vi.resetModules(); vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  const late: Ritual = { id: "late", name: "Late", message: "m", hour: 23, minute: 50, category: "evening", enabled: true };

  it("a 23:50 ritual: handled at 23:55; yesterday's skip neither blocks nor re-logs on the new day", () => {
    expect(Intl.DateTimeFormat().resolvedOptions().timeZone).toBe("Asia/Tokyo");
    expect(planRituals([late], empty(), new Date(2026, 9, 29, 23, 55)).fire?.id).toBe("late");
    // Suppressed (or skipped) at 23:55: recorded for 2026-10-29.
    const st: RitualState = { lastRunTimestamps: {}, skippedDays: { late: "2026-10-29" } };
    expect(planRituals([late], st, new Date(2026, 9, 29, 23, 58))).toEqual({ fire: null, skip: [] });
    // 00:05 on the 30th: new local day, the 23:50 slot is tonight again. Nothing to fire or skip.
    expect(planRituals([late], st, new Date(2026, 9, 30, 0, 5))).toEqual({ fire: null, skip: [] });
    // The old entry doesn't carry over: tonight's slot fires normally.
    expect(planRituals([late], st, new Date(2026, 9, 30, 23, 52)).fire?.id).toBe("late");
  });

  it("pinned: the window doesn't cross midnight. A 23:50 slot first seen at 00:05 is neither sent nor logged", () => {
    // planRituals only looks at today's slot. Harmless today (the latest ritual is 21:30).
    expect(planRituals([late], empty(), new Date(2026, 9, 30, 0, 5))).toEqual({ fire: null, skip: [] });
  });

  it("real list: the day's skips logged at 23:55 are not re-logged at 00:05", async () => {
    vi.setSystemTime(new Date(2026, 9, 29, 23, 55));
    await tick();
    const first = m.bus.skip.mock.calls.length;
    expect(first).toBeGreaterThan(0);
    vi.resetModules(); // relaunch across midnight with the persisted state
    vi.setSystemTime(new Date(2026, 9, 30, 0, 5));
    await tick();
    expect(m.bus.skip.mock.calls.length).toBe(first);
    expect(m.sends).toEqual([]);
  });
});

describe("restart with the persisted state", () => {
  beforeEach(() => { m.kv.clear(); m.sends = []; vi.clearAllMocks(); vi.resetModules(); vi.useFakeTimers(); });
  afterEach(() => { m.refuse = null; vi.useRealTimers(); });

  it("a skip is recorded once across a relaunch", async () => {
    vi.setSystemTime(at(10, 39));
    await tick();
    expect(m.bus.skip).toHaveBeenCalledTimes(3);
    const saved = m.kv.get("ritual_timestamps")!;
    expect((JSON.parse(saved) as RitualState).skippedDays).toBeDefined();
    vi.resetModules();
    vi.setSystemTime(at(10, 44));
    await tick();
    expect(m.bus.skip).toHaveBeenCalledTimes(3);
    expect(m.bus.register).toHaveBeenCalledTimes(3);
  });

  it("a Suppressed send is recorded once across a relaunch", async () => {
    m.refuse = "Outbound sends are disabled for this session (NEXUS_OUTBOUND_DISABLED).";
    vi.setSystemTime(at(8, 2));
    await tick();
    vi.resetModules();
    vi.setSystemTime(at(8, 7));
    await tick();
    expect(outcomes("Presence Drill")).toEqual([["suppress", "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)"]]);
  });
});

describe("failure backoff vs the window: one follow-up per failure, never a retry and a skip", () => {
  beforeEach(() => { m.kv.clear(); m.sends = []; vi.clearAllMocks(); vi.resetModules(); vi.useFakeTimers(); });
  afterEach(() => { m.refuse = null; vi.useRealTimers(); });

  const run = async (times: [number, number][], refuseUntil: number) => {
    for (const [i, [h, min]] of times.entries()) {
      m.refuse = i < refuseUntil ? "POST https://api.telegram.org/botXYZ failed" : null;
      vi.setSystemTime(at(h, min));
      await tick();
    }
    return outcomes("Presence Drill");
  };

  it("fails at +25, retried at +30 (inside the window) and sent: no skip", async () => {
    expect(await run([[8, 25], [8, 30], [8, 35], [8, 40]], 1)).toEqual([
      ["fail", "delivery failed"],
      ["complete", "Sent: Presence Drill"],
    ]);
  });

  it("fails at +25, retry at +30 fails too: the window then closes without a second 'missed' entry", async () => {
    expect(await run([[8, 25], [8, 30], [8, 35], [8, 40]], 4)).toEqual([
      ["fail", "delivery failed"],
      ["fail", "delivery failed"],
    ]);
    const st = JSON.parse(m.kv.get("ritual_timestamps") ?? "{}") as RitualState;
    expect(st.skippedDays?.["presence-drill"]).toBe("2026-10-29"); // done for today
  });

  it("fails at +27; the backoff ends after the window: one skip, no retry", async () => {
    expect(await run([[8, 27], [8, 32], [8, 37]], 3)).toEqual([
      ["fail", "delivery failed"],
      ["skip", "Skipped (missed window)"],
    ]);
  });
});
