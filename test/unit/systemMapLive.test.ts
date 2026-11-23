import { describe, expect, it, vi } from "vitest";
import { makeLiveSystemMap } from "../../src-console/standalone/systemMapLive";
import type { SystemMap } from "../../src-console/standalone/systemMap";

const map = (stamp: string, status: "ok" | "broken" = "ok"): SystemMap => ({
  nodes: [{ id: "app", label: "Amara OS", kind: "app" }],
  paths: [{ id: "app-router", from: "app", to: "router", carries: "x", status, lastOkAt: null, reason: null, fix: null }],
  checkedAt: stamp,
});

describe("live system map adapter", () => {
  it("is unwired until the first successful refresh, then live", async () => {
    const adapter = makeLiveSystemMap(async () => map("t1"));
    expect(adapter.map().state).toBe("unwired");
    await adapter.refresh();
    const w = adapter.map();
    expect(w.state).toBe("live");
    expect(w.state === "live" && w.value.checkedAt).toBe("t1");
  });

  it("stays unwired when the first refresh fails", async () => {
    const adapter = makeLiveSystemMap(async () => { throw new Error("boom"); });
    await expect(adapter.refresh()).resolves.toBeUndefined();
    expect(adapter.map().state).toBe("unwired");
  });

  it("keeps the last good map when a refresh fails or returns junk", async () => {
    const fetchMap = vi.fn<() => Promise<SystemMap>>();
    fetchMap.mockResolvedValueOnce(map("good"));
    const adapter = makeLiveSystemMap(fetchMap);
    await adapter.refresh();
    fetchMap.mockRejectedValueOnce(new Error("down"));
    await adapter.refresh();
    fetchMap.mockResolvedValueOnce({} as SystemMap);
    await adapter.refresh();
    const w = adapter.map();
    expect(w.state === "live" && w.value.checkedAt).toBe("good");
  });

  it("replaces the cache with a newer map, including a broken one", async () => {
    const fetchMap = vi.fn<() => Promise<SystemMap>>();
    fetchMap.mockResolvedValueOnce(map("a")).mockResolvedValueOnce(map("b", "broken"));
    const adapter = makeLiveSystemMap(fetchMap);
    await adapter.refresh();
    await adapter.refresh();
    const w = adapter.map();
    expect(w.state === "live" && w.value.paths[0].status).toBe("broken");
  });

  it("never overlaps refreshes: one in flight, callers share it", async () => {
    let release: (m: SystemMap) => void = () => undefined;
    const fetchMap = vi.fn(() => new Promise<SystemMap>((r) => { release = r; }));
    const adapter = makeLiveSystemMap(fetchMap);
    const a = adapter.refresh();
    const b = adapter.refresh();
    expect(fetchMap).toHaveBeenCalledTimes(1);
    release(map("one"));
    await Promise.all([a, b]);
    const c = adapter.refresh();
    expect(fetchMap).toHaveBeenCalledTimes(2);
    release(map("two"));
    await c;
  });
});
