import { afterEach, describe, expect, it, vi } from "vitest";
import { getAmaraActivity, setAmaraActivity, subscribeAmaraActivity } from "./amaraActivity";

describe("amaraActivity store", () => {
  afterEach(() => setAmaraActivity("idle"));
  it("starts idle and notifies only on change", () => {
    const fn = vi.fn();
    const off = subscribeAmaraActivity(fn);
    expect(getAmaraActivity()).toBe("idle");
    setAmaraActivity("idle");
    expect(fn).not.toHaveBeenCalled();
    setAmaraActivity("thinking");
    expect(getAmaraActivity()).toBe("thinking");
    expect(fn).toHaveBeenCalledTimes(1);
    off();
    setAmaraActivity("idle");
    expect(fn).toHaveBeenCalledTimes(1);
  });
});
