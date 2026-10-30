import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { INGRESS_WORKFLOW_PATH } from "./workflowPath";

// Read-only: "Handle kb.query" from the exported Ingress (INGRESS_WORKFLOW_PATH),
// run with the globals n8n's task runner gives a Code node ($input, helpers), Verba faked.
const wf = JSON.parse(readFileSync(INGRESS_WORKFLOW_PATH, "utf8")) as {
  nodes: { name: string; parameters: { jsCode?: string } }[];
};
const code = wf.nodes.find((n) => n.name === "Handle kb.query")!.parameters.jsCode!;
const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
const handler = new AsyncFunction("$input", "helpers", code) as (i: unknown, h: unknown) => Promise<{ json: any }[]>;

type Request = { url: string; timeout: number; body: any };
async function kbQuery(httpRequest: (r: Request) => Promise<unknown>, payload: Record<string, unknown> = { query: "nexus" }) {
  const body = { trace_id: "t1", event: { type: "kb.query", payload } };
  const [out] = await handler({ first: () => ({ json: { body } }) }, { httpRequest });
  return out.json;
}
const refused = () => Object.assign(new Error("connect ECONNREFUSED 172.18.0.7:8000"), { code: "ECONNREFUSED" });
const GRPC = 'Query failed: Query call with protocol GRPC search failed with message <AioRpcError of RPC that terminated with:\n\tstatus = StatusCode.UNKNOWN\n\tdetails = "object search at index verba_embedding_nomic_embed_text: No properties provided, and no indexed properties found in class"\n\tdebug_error_string = "UNKNOWN:Error received from peer ipv4:172.18.0.4:50051"';

describe("Handle kb.query: Verba down", () => {
  it("connection refused: ok:false 'knowledge base not running', promptly", async () => {
    const t0 = Date.now();
    const out = await kbQuery(async () => { throw refused(); });
    expect(Date.now() - t0).toBeLessThan(500);
    expect(out).toEqual({
      ok: false, trace_id: "t1",
      error: { message: "knowledge base not running", node: "kb.query", status: null, hint: "start it with: docker compose --profile kb up -d" },
    });
  });

  it.each(["ENOTFOUND", "EAI_AGAIN", "ETIMEDOUT"])("%s is also 'knowledge base not running'", async (code) => {
    const out = await kbQuery(async () => { throw Object.assign(new Error(code), { code }); });
    expect(out.error.message).toBe("knowledge base not running");
  });

  it("the Verba call has a bounded timeout", async () => {
    let seen: Request | undefined;
    await kbQuery(async (r) => { seen = r; return { documents: [] }; });
    expect(seen?.url).toBe("http://verba:8000/api/query");
    expect(seen?.timeout).toBeGreaterThan(0);
    expect(seen?.timeout).toBeLessThanOrEqual(30_000);
  });
});

describe("Handle kb.query: Verba 200 with { error }", () => {
  it("empty knowledge base (live 2026-10-30 gRPC text): clean ok:false, no raw text", async () => {
    const out = await kbQuery(async () => ({ error: GRPC, documents: [], context: "" }));
    expect(out).toEqual({ ok: false, trace_id: "t1", error: { message: "knowledge base is empty", node: "kb.query", status: 200 } });
    const s = JSON.stringify(out);
    for (const raw of ["GRPC", "AioRpcError", "debug_error_string", "172.18", "verba_embedding"]) expect(s).not.toContain(raw);
  });

  it("any other Verba error: 'knowledge base query failed'", async () => {
    const out = await kbQuery(async () => ({ error: "Traceback (most recent call last):\n  File \"/Verba/goldenverba/x.py\"" }));
    expect(out).toEqual({ ok: false, trace_id: "t1", error: { message: "knowledge base query failed", node: "kb.query", status: 200 } });
    expect(JSON.stringify(out)).not.toContain("Traceback");
  });

  it("an empty error string is not an error", async () => {
    const out = await kbQuery(async () => ({ error: "  ", documents: [{ title: "a" }] }));
    expect(out).toMatchObject({ ok: true, result: { documents: [{ title: "a" }] } });
  });

  it("a normal result passes through as ok:true", async () => {
    const res = { documents: [{ title: "Nexus", chunks: [] }], context: "c" };
    expect(await kbQuery(async () => res)).toEqual({ ok: true, trace_id: "t1", result: res });
  });

  it("an HTTP error from Verba stays a fixed message with the status only", async () => {
    const out = await kbQuery(async () => { throw Object.assign(new Error("Request failed: secret body"), { httpCode: "500" }); });
    expect(out).toEqual({ ok: false, trace_id: "t1", error: { message: "kb.query failed", node: "kb.query", status: 500 } });
  });
});

describe("Handle kb.query: raw upstream text is logged locally, never returned", () => {
  it("Verba {error}: console.error gets the raw text, the response does not", async () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    const out = await kbQuery(async () => ({ error: GRPC }));
    expect(JSON.stringify(out)).not.toContain("AioRpcError");
    expect(log).toHaveBeenCalledWith(expect.stringContaining("[kb.query]"), expect.stringContaining("AioRpcError"));
  });

  it("connection refused: logged, fixed message returned", async () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    const out = await kbQuery(async () => { throw refused(); });
    expect(out.error.message).toBe("knowledge base not running");
    expect(JSON.stringify(out)).not.toContain("172.18");
    expect(log).toHaveBeenCalledWith(expect.stringContaining("[kb.query]"), expect.stringContaining("ECONNREFUSED"));
  });
});
