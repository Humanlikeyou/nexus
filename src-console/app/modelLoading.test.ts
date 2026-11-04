import { describe, expect, it } from "vitest";
import { MODEL_LOADING_DEFAULT_MS, MODEL_LOADING_MAX_MS, modelLoadingDelay, parseRetryAfter, waitUnlessAborted } from "./modelLoading";

describe("modelLoading", () => {
  it("reads Retry-After seconds, defaults to 3 s, caps at 30 s", () => {
    expect(modelLoadingDelay({ kind: "model_loading", retryAfter: 5 })).toBe(5000);
    expect(modelLoadingDelay({ kind: "model_loading", retry_after: "2" })).toBe(2000);
    expect(modelLoadingDelay({ kind: "model_loading" })).toBe(MODEL_LOADING_DEFAULT_MS);
    expect(modelLoadingDelay({ kind: "model_loading", retryAfter: 9999 })).toBe(MODEL_LOADING_MAX_MS);
    expect(modelLoadingDelay("model_loading: retry_after=300")).toBe(60_000);
    expect(modelLoadingDelay("model_loading: retry_after=10")).toBe(10_000);
    expect(modelLoadingDelay({ kind: "model_loading", retryAfter: "Wed, 21 Oct 2026 07:28:00 GMT" })).toBe(MODEL_LOADING_DEFAULT_MS);
    expect(modelLoadingDelay("model_loading: Model loading retry_after=4")).toBe(4000);
    expect(modelLoadingDelay(new Error("Hermes is busy with another request. Try again in a moment."))).toBe(MODEL_LOADING_DEFAULT_MS);
  });
  it("other errors are not retried", () => {
    expect(modelLoadingDelay(new Error("Hermes could not complete the request."))).toBeNull();
    expect(modelLoadingDelay({ kind: "other" })).toBeNull();
    expect(parseRetryAfter("-1")).toBeNull();
  });
  it("wait resolves false when aborted", async () => {
    const c = new AbortController();
    const p = waitUnlessAborted(60_000, c.signal);
    c.abort();
    await expect(p).resolves.toBe(false);
    await expect(waitUnlessAborted(1, new AbortController().signal)).resolves.toBe(true);
  });
});
