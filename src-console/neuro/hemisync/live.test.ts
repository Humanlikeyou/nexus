import { describe, expect, it, vi } from "vitest";
import { hemisyncSrc, makeLiveHemisync, mapLibrary, mapSession, type HemisyncDeps } from "./live";
import type { ImportProgress } from "./adapters";

const raw = {
  tracks: [
    { id: "b", wave: 5, waveName: "Exploring", order: 2, title: "Two", durationMs: null },
    { id: "a", wave: 1, waveName: "Discovery", order: 1, title: "One", durationMs: null },
    { id: "c", wave: 5, waveName: "Exploring", order: 1, title: "First", durationMs: null },
  ],
  importedAt: "2026-11-12T10:00:00+09:00", sourceFolder: "~/Desktop/Hemisync Collection",
};
const rawSession = { id: "s1", trackId: "a", startedAt: "2026-11-12T21:00:00+09:00", listenedMs: 60000, completed: true, rating: 4, note: null, metrics: { focusAvg: 62, calmAvg: null, bands: null } };

function deps(over: Partial<Record<string, (args?: Record<string, unknown>) => unknown>> = {}) {
  const calls: Array<[string, Record<string, unknown> | undefined]> = [];
  let emit: (p: ImportProgress) => void = () => undefined;
  const handlers: Record<string, (args?: Record<string, unknown>) => unknown> = {
    hemisync_library: () => raw,
    hemisync_sessions: () => [rawSession],
    hemisync_stats: () => [{ trackId: "a", sessions: 1, lastAt: rawSession.startedAt, avgRating: 4, focusAvg: 62, calmAvg: null }],
    hemisync_import: () => ({ added: 2, skipped: 1 }),
    hemisync_session_link: () => undefined, hemisync_session_finish: () => undefined, hemisync_session_rate: () => undefined,
    ...over,
  };
  const d: HemisyncDeps = {
    call: (async (c: string, a?: Record<string, unknown>) => { calls.push([c, a]); const h = handlers[c]; if (!h) throw new Error("x"); return h(a); }) as HemisyncDeps["call"],
    toSrc: (id) => `hemisync://localhost/${id}`,
    listenImport: (cb) => { emit = cb; },
    notify: vi.fn(),
  };
  return { d, calls, emit: (p: ImportProgress) => emit(p) };
}
const settle = () => new Promise((r) => setTimeout(r, 0));

describe("hemisync live mapping", () => {
  it("groups waves, orders tracks and fills src from the id lookup", () => {
    const lib = mapLibrary(raw, { a: "u-a", b: "u-b" });
    expect(lib.waves.map((w) => w.wave)).toEqual([1, 5]);
    expect(lib.waves[1].tracks.map((t) => t.id)).toEqual(["c", "b"]);
    expect(lib.waves[0].tracks[0].src).toBe("u-a");
    expect(lib.waves[1].tracks[0].src).toBe("");
    expect(lib.sourceFolder).toBe("~/Desktop/Hemisync Collection");
  });
  it("keeps null metrics null and rejects out-of-range ratings", () => {
    const s = mapSession({ ...rawSession, rating: 9, metrics: { focusAvg: Number.NaN, calmAvg: null, bands: null } });
    expect(s.rating).toBeNull();
    expect(s.metrics).toEqual({ focusAvg: null, calmAvg: null, bands: null });
    expect(mapSession(rawSession).metrics.focusAvg).toBe(62);
  });
});

describe("hemisyncSrc", () => {
  it("builds id-only URLs per platform and refuses anything that is not a track id", () => {
    expect(hemisyncSrc("0123456789abcdef", false)).toBe("hemisync://localhost/0123456789abcdef");
    expect(hemisyncSrc("0123456789abcdef", true)).toBe("http://hemisync.localhost/0123456789abcdef");
    for (const bad of ["", "../x", "/etc/passwd", "0123456789ABCDEF", "0123456789abcde", "0123456789abcdef/x", "a?b"]) expect(hemisyncSrc(bad, false)).toBe("");
  });
});

describe("live hemisync adapter", () => {
  it("is unwired until the first read lands, then live from the cache", async () => {
    const { d } = deps();
    const a = makeLiveHemisync(d);
    expect(a.library().state).toBe("unwired");
    await settle();
    const lib = a.library();
    expect(lib.state).toBe("live");
    if (lib.state === "live") expect(lib.value.waves[0].tracks[0].src).toBe("hemisync://localhost/a");
    const s = a.sessions();
    expect(s.state === "live" && s.value[0].rating).toBe(4);
    expect(a.stats().state).toBe("live");
  });
  it("stays unwired when the backend fails", async () => {
    const boom = () => { throw new Error("no"); };
    const { d } = deps({ hemisync_library: boom, hemisync_sessions: boom, hemisync_stats: boom });
    const a = makeLiveHemisync(d);
    a.library();
    await settle();
    expect(a.library().state).toBe("unwired");
    expect(a.sessions().state).toBe("unwired");
    expect((await a.rate({ sessionId: "s", rating: 3 })).state).toBe("live"); // write handler exists
  });
  it("write failures are unwired and send camelCase args", async () => {
    const { d, calls } = deps({ hemisync_session_finish: () => { throw new Error("no"); } });
    const a = makeLiveHemisync(d);
    expect((await a.linkSession({ sessionId: "s1", trackId: "a", startedAt: "t" })).state).toBe("live");
    expect(calls.find(([c]) => c === "hemisync_session_link")?.[1]).toEqual({ sessionId: "s1", trackId: "a", startedAt: "t" });
    expect((await a.finishSession({ sessionId: "s1", listenedMs: 5, completed: true })).state).toBe("unwired");
    await a.rate({ sessionId: "s1", rating: 5, note: "ok" });
    expect(calls.find(([c]) => c === "hemisync_session_rate")?.[1]).toEqual({ sessionId: "s1", rating: 5, note: "ok" });
  });
  it("import returns counts, reports progress from the event and clears it", async () => {
    const { d, emit, calls } = deps();
    const a = makeLiveHemisync(d);
    a.library();
    emit({ copied: 1, total: 3, phase: "copying" });
    expect(a.importProgress()).toEqual({ copied: 1, total: 3, phase: "copying" });
    const r = await a.importFrom();
    expect(r).toEqual({ state: "live", value: { added: 2, skipped: 1 } });
    expect(calls.find(([c]) => c === "hemisync_import")?.[1]).toEqual({ folder: null });
    expect(a.importProgress()).toBeNull();
  });
  it("a failed import is unwired", async () => {
    const { d } = deps({ hemisync_import: () => { throw new Error("not_enough_space"); } });
    expect((await makeLiveHemisync(d).importFrom("~/x")).state).toBe("unwired");
  });
});
