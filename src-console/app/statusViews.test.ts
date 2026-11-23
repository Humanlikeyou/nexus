import { describe, expect, it } from "vitest";
import { crewSnapshot, pickKb } from "../bridge";
import { kbView } from "./statusViews";

const kb = (state: "running" | "empty" | "not_running" | "error", over = {}) => ({ state, model: "nomic-embed-text", docs: 0, detail: null, ...over });

describe("kbView: fixed copy per state", () => {
  it("running: Ready with the document count and model", () => {
    expect(kbView(kb("running", { docs: 8 }), false)).toEqual({ tone: "success", label: "Ready", body: "8 documents · nomic-embed-text", retry: false });
    expect(kbView(kb("running", { docs: 1, model: null }), false).body).toBe("1 document");
  });
  it("empty: neutral, 'Knowledge base is empty' + help line", () => {
    expect(kbView(kb("empty"), false)).toEqual({ tone: "neutral", label: "Empty", title: "Knowledge base is empty", body: "Add documents to search them.", retry: false });
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
