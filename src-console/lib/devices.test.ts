import { describe, expect, it } from "vitest";
import { EMPTY_DEVICE_ID, deviceTitle } from "./devices";

describe("deviceTitle (Decision 10: the placeholder id never shows)", () => {
  it("placeholder, empty or missing → 'WHOOP 4' (design-1111s: no NOOP framing)", () => {
    expect(deviceTitle(EMPTY_DEVICE_ID)).toBe("WHOOP 4");
    expect(deviceTitle("")).toBe("WHOOP 4");
    expect(deviceTitle(null)).toBe("WHOOP 4");
    expect(deviceTitle(undefined)).toBe("WHOOP 4");
  });
  it("a real id is shown as is", () => expect(deviceTitle("noop-1")).toBe("noop-1"));
});
