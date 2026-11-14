// The 6 bridge functions backed by generated bindings (src-console/bindings.gen.ts):
// same command name, same payload, same resolve/reject behaviour as the old hand-written invoke.
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), Channel: class {} }));

import { amaraTurnCancel, getN8nStatus, loadKv, recordScreenOpen, saveKv, screenOpenCounts } from "../../src-console/bridge";

beforeEach(() => { invoke.mockReset(); });
const sent = () => invoke.mock.calls.map((c) => c.slice(0, 2));

describe("bridge functions on generated bindings", () => {
  it("loadKv", async () => {
    invoke.mockResolvedValue("v");
    await expect(loadKv("k")).resolves.toBe("v");
    expect(sent()).toEqual([["load_kv", { key: "k" }]]);
    invoke.mockRejectedValue("reserved_key");
    await expect(loadKv("k")).rejects.toBe("reserved_key");
  });

  it("saveKv resolves with the raw backend value and rejects on error", async () => {
    invoke.mockResolvedValue(null);
    await expect(saveKv("k", "v")).resolves.toBeNull();
    expect(sent()).toEqual([["save_kv", { key: "k", value: "v" }]]);
    invoke.mockRejectedValue("disk full");
    await expect(saveKv("k", "v")).rejects.toBe("disk full");
  });

  it("recordScreenOpen swallows errors", async () => {
    invoke.mockResolvedValue(null);
    await recordScreenOpen("life");
    expect(sent()).toEqual([["screen_open_record", { screen: "life" }]]);
    invoke.mockRejectedValue("no backend");
    await expect(recordScreenOpen("life")).resolves.toBeUndefined();
  });

  it("screenOpenCounts sends weeks (null when omitted) and surfaces errors", async () => {
    invoke.mockResolvedValue([]);
    await screenOpenCounts(4);
    await screenOpenCounts();
    expect(sent()).toEqual([["screen_open_counts", { weeks: 4 }], ["screen_open_counts", { weeks: null }]]);
    invoke.mockRejectedValue("store_error");
    await expect(screenOpenCounts()).rejects.toBe("store_error");
  });

  it("getN8nStatus sends no args", async () => {
    invoke.mockResolvedValue({ connected: false, workflows: null, failures7d: null });
    await expect(getN8nStatus()).resolves.toEqual({ connected: false, workflows: null, failures7d: null });
    expect(sent()).toEqual([["n8n_status", undefined]]);
    invoke.mockRejectedValue("boom");
    await expect(getN8nStatus()).rejects.toBe("boom");
  });

  it("amaraTurnCancel sends turnId (camelCase)", async () => {
    invoke.mockResolvedValue(true);
    await expect(amaraTurnCancel("t1")).resolves.toBe(true);
    expect(sent()).toEqual([["amara_turn_cancel", { turnId: "t1" }]]);
    invoke.mockRejectedValue("x");
    await expect(amaraTurnCancel("t1")).rejects.toBe("x");
  });
});
