import { describe, expect, it } from "vitest";
import { crewSnapshot, pickKb } from "../bridge";
import type { OpenClawStatus } from "../types";
import { assistantView, BACKEND_UNREACHABLE_COPY, kbView } from "./statusViews";

const kb = (state: "running" | "empty" | "not_running" | "error", over = {}) => ({ state, model: "nomic-embed-text", docs: 0, detail: null, ...over });

describe("kbView: fixed copy per state", () => {
  it("running: Ready with the document count and model", () => {
    expect(kbView(kb("running", { docs: 8 }), false)).toEqual({ tone: "success", label: "Ready", body: "8 documents · nomic-embed-text", retry: false });
    expect(kbView(kb("running", { docs: 1, model: null }), false).body).toBe("1 document");
  });
  it("empty: neutral, 'knowledge base is empty'", () => {
    expect(kbView(kb("empty"), false)).toEqual({ tone: "neutral", label: "Empty", body: "knowledge base is empty", retry: false });
  });
  it("not_running: warning 'not running' with Retry", () => {
    expect(kbView(kb("not_running"), false)).toEqual({ tone: "warning", label: "Unavailable", body: "not running", retry: true });
  });
  it("error: danger 'query failed' with Retry", () => {
    expect(kbView(kb("error"), false)).toEqual({ tone: "danger", label: "Error", body: "query failed", retry: true });
  });
  it("command unavailable: warning with Retry, whatever the last state", () => {
    expect(kbView(kb("running", { docs: 3 }), true)).toMatchObject({ tone: "warning", retry: true });
  });
});

describe("pickKb: nothing raw reaches the UI", () => {
  it("drops detail and unknown states, keeps plain numbers and model ids", () => {
    const raw = "AioRpcError StatusCode.UNKNOWN debug_error_string 172.18.0.4";
    expect(pickKb({ state: "error", detail: raw, model: "nomic-embed-text", docs: 2 })).toEqual({ state: "error", model: "nomic-embed-text", docs: 2, detail: null });
    expect(pickKb({ state: "weird" as never })).toMatchObject({ state: "error" });
    expect(pickKb({ state: "running", model: `bad <script>${raw}`, docs: -1 })).toEqual({ state: "running", model: null, docs: null, detail: null });
    expect(JSON.stringify(pickKb({ state: "error", detail: raw }))).not.toContain("AioRpcError");
  });
});

describe("crewSnapshot tools", () => {
  it("keeps name + allowed only; drops anything that isn't a tool name", () => {
    const tools = crewSnapshot("operational", [], [
      { name: "kb_query", allowed: true },
      { name: "memory_query", allowed: false },
      { name: "http://x?key=SECRET", allowed: true },
      { name: "memory_query", allowed: "yes" as never, extra: "sk-x" } as never,
    ]).tools;
    expect(tools).toEqual([{ name: "kb_query", allowed: true }, { name: "memory_query", allowed: false }, { name: "memory_query", allowed: false }]);
    expect(JSON.stringify(tools)).not.toMatch(/SECRET|sk-x|http/);
  });
  it("defaults to no tools", () => {
    expect(crewSnapshot("operational", []).tools).toEqual([]);
    expect(crewSnapshot("operational", [], null as never).tools).toEqual([]);
  });
});

describe("assistantView", () => {
  const s = (status: OpenClawStatus["status"], model = "unknown"): OpenClawStatus => ({ status, online: status === "online", model, gateway_url: "", last_heartbeat: null });
  it("loading or unreadable: neutral 'Assistant'", () => {
    expect(assistantView(s("online"), { loading: true, unavailable: false })).toMatchObject({ kind: "row", title: "Assistant", tone: "neutral" });
    expect(assistantView(s("offline"), { loading: false, unavailable: true })).toMatchObject({ kind: "row", title: "Assistant", tone: "neutral" });
  });
  it("reachable: 'Amara' with its model", () => {
    expect(assistantView(s("online", "minimax-m2.5"), { loading: false, unavailable: false })).toEqual({ kind: "row", title: "Amara", tone: "success", label: "Online", subtitle: "minimax-m2.5" });
    expect(assistantView(s("online"), { loading: false, unavailable: false })).toMatchObject({ subtitle: "Backend reachable" });
  });
  it("installed but silent: Assistant backend not reachable", () => {
    expect(assistantView(s("configured"), { loading: false, unavailable: false })).toEqual({ kind: "unreachable", message: BACKEND_UNREACHABLE_COPY });
    expect(BACKEND_UNREACHABLE_COPY).toBe("Assistant backend not reachable");
  });
  it.each(["not_installed", "offline"] as const)("%s (local mode): hidden", (st) => {
    expect(assistantView(s(st), { loading: false, unavailable: false })).toBeNull();
  });
  it("never says OpenClaw", () => {
    for (const st of ["online", "configured"] as const) expect(JSON.stringify(assistantView(s(st), { loading: false, unavailable: false }))).not.toMatch(/openclaw/i);
  });
});
