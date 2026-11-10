import { describe, expect, it } from "vitest";
import * as recordings from "./recordings";
import { createSampleRecordings, isExpiring, liveRecordings, recordingFromSession, sampleTrace } from "./recordings";

const NOW = Date.parse("2026-11-10T18:00:00+09:00");
const s = (id: string, start: string, file = true) => ({ id, device_type: "muse", start, end: start, duration_secs: 60, data_points: 0, file_path: file ? `/x/${id}.ndjson` : undefined, summary: { avg_focus: 0.3, avg_relaxation: 50 } });

describe("design-1110e recordings adapters", () => {
  it("live: only facts from the session files; every backend piece is unwired", () => {
    const a = liveRecordings([s("a", "2026-11-07T11:57:00+09:00"), s("b", "2026-02-18T22:51:00+09:00", false)], NOW);
    const list = a.list();
    expect(list.state).toBe("live");
    if (list.state !== "live") return;
    expect(list.value.map((r) => [r.channels.length, r.raw, r.retention, r.avgFocus])).toEqual([[4, "available", "kept", 30], [4, "summary-only", "expiring", 30]]);
    for (const w of [a.window("a", 0, 5), a.body("a", 0, 5), a.series("a"), a.contact("a"), a.liveAux()]) expect(w.state).toBe("unwired");
    expect(a.removeConfirmed(["b"], { confirmedByUser: true, at: "x" }).state).toBe("unwired");
  });

  it("retention is computed from dates (90 days) and nothing in the adapters can reach the disk", () => {
    expect(isExpiring("2026-08-11T00:00:00+09:00", NOW)).toBe(true);
    expect(isExpiring("2026-08-13T00:00:00+09:00", NOW)).toBe(false);
    expect(recordingFromSession(s("c", "2026-03-05T19:52:00+09:00"), NOW).retention).toBe("expiring");
    // No export can delete: the only removal path is the adapter method, unwired in live.
    expect(Object.keys(recordings).filter((k) => /delete|purge|remove|unlink/i.test(k))).toEqual([]);
  });

  it("sample: removal refuses without the user's confirmation and never claims a real delete", () => {
    const a = createSampleRecordings();
    expect(() => a.removeConfirmed(["r7"], { confirmedByUser: false } as never)).toThrow(/confirmation/);
    expect(a.removals).toHaveLength(0);
    const r = a.removeConfirmed(["r7"], { confirmedByUser: true, at: "now" });
    expect(r.state).toBe("sample");
  });

  it("sample traces are deterministic, smoothed to the point budget, and in a plausible µV range", () => {
    const t = sampleTrace("TP9", 10, 5, 900);
    expect(t).toHaveLength(900);
    expect(sampleTrace("TP9", 10, 5, 900)).toEqual(t);
    expect(Math.max(...t.map(Math.abs))).toBeLessThan(100);
  });
});
