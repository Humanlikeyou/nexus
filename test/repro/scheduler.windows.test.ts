/**
 * Table-driven window tests for the pure scheduler API (TZ pinned to Asia/Tokyo by test:unit).
 */
import { describe, it, expect, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn() }));
vi.mock("../../src/terrance/system/continuity", () => ({}));
vi.mock("../../src/terrance/system/selfDiagnosis", () => ({}));
vi.mock("../../src/terrance/system/rituals", () => ({}));
vi.mock("../../src/terrance/system/customJobs", () => ({}));
vi.mock("../../src/terrance/system/skillsEvolution", () => ({}));
vi.mock("../../src/terrance/system/subagents", () => ({}));
vi.mock("../../src/terrance/system/telegramBridge", () => ({}));
vi.mock("../../src/terrance/system/activityBus", () => ({ activityBus: {} }));

import {
  decideDueTasks, lastSlot, isTouchDue, type SchedulerTimestamps,
} from "../../src/terrance/system/scheduler";

const H = 3_600_000;
const T = (local: string) => new Date(`${local}+09:00`).getTime();
const YDAY = "2026-10-20T";
const TODAY = "2026-10-21T";

/** Everything recently done; nothing due. Override per case. */
function done(now: number, over: Partial<SchedulerTimestamps> = {}): SchedulerTimestamps {
  return {
    lastDailySummaryTimestamp: now - 1,
    lastMiddayTimestamp: now - 1,
    lastEveningTimestamp: now - 1,
    lastHealthCheckTimestamp: now,
    lastConsolidationTimestamp: now,
    lastReasoningTimestamp: now,
    lastPruneTimestamp: now,
    ...over,
  };
}
const ids = (ctx: SchedulerTimestamps, now: number) => decideDueTasks(ctx, now).map((t) => t.id);

describe("lastSlot", () => {
  it.each([
    ["06:59 → yesterday 07:00", `${TODAY}06:59:00`, 7, 0, `${YDAY}07:00:00`],
    ["07:00 exactly → today", `${TODAY}07:00:00`, 7, 0, `${TODAY}07:00:00`],
    ["12:29 → yesterday 12:30", `${TODAY}12:29:59`, 12, 30, `${YDAY}12:30:00`],
    ["00:30 → yesterday 21:00", `2026-10-22T00:30:00`, 21, 0, `${TODAY}21:00:00`],
  ])("%s", (_n, now, h, m, want) => {
    expect(lastSlot(T(now), h, m)).toBe(T(want));
  });
});

describe("isTouchDue", () => {
  it("missing or zero last is never due (scheduler seeds instead)", () => {
    expect(isTouchDue(undefined, T(`${TODAY}07:00:00`), 7, 0, H).due).toBe(false);
    expect(isTouchDue(0, T(`${TODAY}07:00:00`), 7, 0, H).due).toBe(false);
  });
  it("on-time is not catch-up; late inside the window is", () => {
    const last = T(`${YDAY}07:00:00`);
    expect(isTouchDue(last, T(`${TODAY}07:05:00`), 7, 0, 5 * H)).toMatchObject({ due: true, catchUp: false });
    expect(isTouchDue(last, T(`${TODAY}09:00:00`), 7, 0, 5 * H)).toMatchObject({ due: true, catchUp: true });
  });
  it("window end is exclusive", () => {
    const last = T(`${YDAY}07:00:00`);
    expect(isTouchDue(last, T(`${TODAY}08:00:00`), 7, 0, H).due).toBe(false);
  });
});

describe("decideDueTasks: daily touches", () => {
  type Row = [name: string, now: string, over: Partial<SchedulerTimestamps>, want: string[], catchUp?: boolean];
  const rows: Row[] = [
    ["daily: due at 07:00 when last ran yesterday", `${TODAY}07:00:00`, { lastDailySummaryTimestamp: T(`${YDAY}07:01:00`) }, ["daily"], false],
    ["daily: not due at 06:59", `${TODAY}06:59:00`, { lastDailySummaryTimestamp: T(`${YDAY}07:01:00`) }, []],
    ["daily: catch-up at 07:29 (inside the 30-min window)", `${TODAY}07:29:00`, { lastDailySummaryTimestamp: T(`${YDAY}07:01:00`) }, ["daily"], true],
    ["daily: 07:30 and later is skipped, no catch-up (2026-10-29)", `${TODAY}11:00:00`, { lastDailySummaryTimestamp: T(`${YDAY}07:01:00`) }, []],
    ["daily: catch-up ends at 12:30", `${TODAY}12:30:00`, { lastDailySummaryTimestamp: T(`${YDAY}07:01:00`), lastMiddayTimestamp: T(`${TODAY}12:30:00`) }, []],
    ["daily: cooldown, already ran today", `${TODAY}09:00:00`, { lastDailySummaryTimestamp: T(`${TODAY}07:02:00`) }, []],
    ["daily: custom hour 9", `${TODAY}09:00:00`, { dailySummaryHour: 9, lastDailySummaryTimestamp: T(`${YDAY}09:01:00`) }, ["daily"], false],
    ["daily: invalid hour 25 falls back to 7", `${TODAY}07:00:00`, { dailySummaryHour: 25, lastDailySummaryTimestamp: T(`${YDAY}07:01:00`) }, ["daily"], false],
    ["midday: not due at 12:29", `${TODAY}12:29:00`, { lastMiddayTimestamp: T(`${YDAY}12:31:00`) }, []],
    ["midday: due at 12:30", `${TODAY}12:30:00`, { lastMiddayTimestamp: T(`${YDAY}12:31:00`) }, ["midday"], false],
    ["midday: catch-up at 20:59", `${TODAY}20:59:00`, { lastMiddayTimestamp: T(`${YDAY}12:31:00`) }, ["midday"], true],
    ["midday: window closed at 21:00", `${TODAY}21:00:00`, { lastMiddayTimestamp: T(`${YDAY}12:31:00`), lastEveningTimestamp: T(`${TODAY}21:00:00`) }, []],
    ["midday: cooldown, ran at 12:31", `${TODAY}15:00:00`, { lastMiddayTimestamp: T(`${TODAY}12:31:00`) }, []],
    ["evening: due at 21:00", `${TODAY}21:00:00`, { lastEveningTimestamp: T(`${YDAY}21:01:00`) }, ["evening"], false],
    ["evening: catch-up after midnight (00:30)", `2026-10-22T00:30:00`, { lastEveningTimestamp: T(`${YDAY}21:01:00`) }, ["evening"], true],
    ["evening: catch-up still open at 02:59", `2026-10-22T02:59:00`, { lastEveningTimestamp: T(`${YDAY}21:01:00`) }, ["evening"], true],
    ["evening: window closed at 03:00", `2026-10-22T03:00:00`, { lastEveningTimestamp: T(`${YDAY}21:01:00`) }, []],
    ["evening: cooldown, ran at 21:05", `2026-10-22T00:30:00`, { lastEveningTimestamp: T(`${TODAY}21:05:00`) }, []],
  ];
  it.each(rows)("%s", (_n, nowStr, over, want, catchUp) => {
    const now = T(nowStr);
    const due = decideDueTasks(done(now, over), now);
    expect(due.map((t) => t.id)).toEqual(want);
    if (want.length) expect(due[0].catchUp).toBe(catchUp);
  });

  it("window key is the slot start, so retries are scoped per day", () => {
    const now = T(`${TODAY}07:00:00`);
    const [t] = decideDueTasks(done(now, { lastDailySummaryTimestamp: T(`${YDAY}07:01:00`) }), now);
    expect(t.window).toBe(String(T(`${TODAY}07:00:00`)));
  });
});

describe("decideDueTasks: intervals", () => {
  it.each([
    ["consolidation at exactly 6h", "lastConsolidationTimestamp", 6 * H, ["consolidation"]],
    ["consolidation 1ms short of 6h", "lastConsolidationTimestamp", 6 * H - 1, []],
    ["reasoning at 12h", "lastReasoningTimestamp", 12 * H, ["reasoning"]],
    ["prune at 7d", "lastPruneTimestamp", 7 * 24 * H, ["prune"]],
    ["health at 30m", "lastHealthCheckTimestamp", 30 * 60_000, ["health"]],
  ] as const)("%s", (_n, key, age, want) => {
    const now = T(`${TODAY}15:00:00`);
    expect(ids(done(now, { [key]: now - age }), now)).toEqual(want);
  });
});

describe("decideDueTasks: zeroed / missing timestamps never fire everything", () => {
  const zeros: SchedulerTimestamps = {
    lastDailySummaryTimestamp: 0, lastMiddayTimestamp: 0, lastEveningTimestamp: 0,
    lastHealthCheckTimestamp: 0, lastConsolidationTimestamp: 0, lastReasoningTimestamp: 0, lastPruneTimestamp: 0,
  };
  it.each([`${TODAY}07:00:00`, `${TODAY}13:00:00`, `${TODAY}21:30:00`, `2026-10-22T00:30:00`])("all zero at %s → only health", (n) => {
    expect(ids(zeros, T(n))).toEqual(["health"]);
  });
  it.each([`${TODAY}07:00:00`, `${TODAY}13:00:00`])("empty context at %s → only health", (n) => {
    expect(ids({}, T(n))).toEqual(["health"]);
  });
});
