import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

const FAKE = "FAKE_TOKEN_NOT_REAL";
const hostile = new Error(`BLE scan failed at /Users/someone/.config/x ${FAKE}`);

vi.mock("@/lib/electrode/myndliftAdapter", () => ({
  StubMyndliftAdapter: class {
    async scan() { throw hostile; }
  },
}));

import { useElectrode, ELECTRODE_SCAN_ERROR } from "./useElectrode";

describe("useElectrode scan error", () => {
  beforeEach(() => vi.spyOn(console, "error").mockImplementation(() => {}));

  it("sets fixed lastError text, never the raw error, and logs the raw error locally", async () => {
    const { result } = renderHook(() => useElectrode(false, false, "demo" as never, 0));
    await act(async () => {
      await result.current.scanForDevices();
    });
    expect(result.current.state.lastError).toBe(ELECTRODE_SCAN_ERROR);
    expect(result.current.state.lastError).not.toContain("/Users/");
    expect(result.current.state.lastError).not.toContain(FAKE);
    expect(console.error).toHaveBeenCalledWith("[electrode] scan failed:", hostile);
    expect(result.current.scanning).toBe(false);
  });
});
