import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

describe("live crews next run", () => {
  beforeEach(() => { vi.resetModules(); invoke.mockReset(); });

  it("picks the earliest enabled active nextRunAt", async () => {
    const { nextScheduleFrom } = await import("./liveSchedule");
    const r = nextScheduleFrom([
      { enabled: true, status: "active", nextRunAt: "2026-11-14T09:00:00Z" },
      { enabled: true, status: "active", nextRunAt: "2026-11-13T09:00:00Z" },
      { enabled: false, status: "active", nextRunAt: "2026-11-12T09:00:00Z" },
      { enabled: true, status: "paused", nextRunAt: "2026-11-12T10:00:00Z" },
    ]);
    expect(r.state).toBe("live");
    if (r.state === "live") expect(r.value).toContain("13");
  });

  it("none scheduled is an honest empty line", async () => {
    const { nextScheduleFrom } = await import("./liveSchedule");
    expect(nextScheduleFrom([{ enabled: true, status: "active", nextRunAt: null }])).toEqual({ state: "live", value: "none scheduled" });
    expect(nextScheduleFrom([])).toEqual({ state: "live", value: "none scheduled" });
  });

  it("is unwired on a bad shape or a failed read", async () => {
    const { nextScheduleFrom, makeLiveCrewsSchedule, refreshNextSchedule } = await import("./liveSchedule");
    const { unwiredCrews } = await import("./adapters");
    expect(nextScheduleFrom("nope").state).toBe("unwired");
    invoke.mockRejectedValue("boom");
    const a = makeLiveCrewsSchedule(unwiredCrews);
    await refreshNextSchedule();
    expect(a.nextSchedule().state).toBe("unwired");
  });
});
