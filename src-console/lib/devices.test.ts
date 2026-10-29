import { describe, expect, it } from "vitest";
import { EMPTY_DEVICE_ID, deviceTitle } from "./devices";

describe("deviceTitle (Decision 10: the placeholder id never shows)", () => {
  it("placeholder, empty or missing → 'NOOP band'", () => {
    expect(deviceTitle(EMPTY_DEVICE_ID)).toBe("NOOP band");
    expect(deviceTitle("")).toBe("NOOP band");
    expect(deviceTitle(null)).toBe("NOOP band");
    expect(deviceTitle(undefined)).toBe("NOOP band");
  });
  it("a real id is shown as is", () => expect(deviceTitle("noop-1")).toBe("noop-1"));
});
