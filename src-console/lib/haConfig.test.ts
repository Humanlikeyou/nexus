import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { haConfigBackend, isCommandMissing, saveErrorCopy, testResultCopy, tokenError, urlError } from "./haConfig";

beforeEach(() => invoke.mockReset());

describe("urlError (loopback only)", () => {
  it.each(["http://127.0.0.1:8123", "https://localhost", "http://[::1]:8123/", "HTTP://LOCALHOST:1"])("accepts %s", (u) => expect(urlError(u)).toBeNull());
  it.each(["", "http://example.com", "http://192.168.1.2:8123", "ftp://127.0.0.1", "http://user:pw@127.0.0.1", "http://127.0.0.1:8123/api", "http://127.0.0.1:0", "http://127.0.0.1:70000", "http://127.0.0.1.evil.com", "http://localhost?x=1"])("rejects %s", (u) => expect(urlError(u)).not.toBeNull());
});

describe("wrapper", () => {
  it("getConfig picks only url and tokenSet (never a token)", async () => {
    invoke.mockResolvedValue({ url: "http://127.0.0.1:8123", tokenSet: true, token: "leak" });
    const c = await haConfigBackend().getConfig();
    expect(c).toEqual({ url: "http://127.0.0.1:8123", tokenSet: true });
    expect(JSON.stringify(c)).not.toContain("leak");
  });
  it("setConfig sends only provided keys", async () => {
    invoke.mockResolvedValue(undefined);
    await haConfigBackend().setConfig({ token: "t" });
    expect(invoke).toHaveBeenCalledWith("ha_set_config", { token: "t" });
    await haConfigBackend().setConfig({ url: "http://localhost" });
    expect(invoke).toHaveBeenLastCalledWith("ha_set_config", { url: "http://localhost" });
  });
  it("clearToken and testConnection call their commands", async () => {
    invoke.mockResolvedValue({ ok: false, kind: "timed_out", extra: 1 });
    expect(await haConfigBackend().testConnection()).toEqual({ ok: false, kind: "timed_out", version: undefined });
    await haConfigBackend().clearToken();
    expect(invoke).toHaveBeenLastCalledWith("ha_clear_token");
  });
});

describe("copy", () => {
  it("detects a missing command", () => {
    expect(isCommandMissing("Command ha_get_config not found")).toBe(true);
    expect(isCommandMissing("auth_rejected: nope")).toBe(false);
  });
  it("maps save errors without echoing the message", () => {
    expect(saveErrorCopy("bad_url: secret-ish detail")).toMatch(/local address/);
    expect(saveErrorCopy("invalid_input: abc123")).not.toContain("abc123");
    expect(saveErrorCopy(new Error("boom tok"))).not.toContain("tok");
  });
  it.each([["not_configured", "Add the address"], ["auth_rejected", "rejected the token"], ["unreachable", "isn't reachable"], ["timed_out", "too long"], ["upstream_error", "returned an error"], ["weird", "Couldn't test"]])("test kind %s", (kind, text) => {
    expect(testResultCopy({ ok: false, kind })).toContain(text);
  });
  it("token validation", () => {
    expect(tokenError("")).toBeNull();
    expect(tokenError("a b")).not.toBeNull();
    expect(tokenError("abc.def")).toBeNull();
  });
});
