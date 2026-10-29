import { describe, it, expect, vi, afterEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/lib/api", () => ({ getConfig: vi.fn() }));
vi.mock("@/lib/llm-router", () => ({ buildRouterConfig: vi.fn() }));
vi.mock("./deliberation", () => ({ deliberate: vi.fn() }));

import { MEMORY_WRITE_MAX_CHARS, memoryWriteFailure, postMemoryWrite } from "./continuity";

function respond(status: number, body: unknown) {
  return vi.fn(async () => new Response(typeof body === "string" ? body : JSON.stringify(body), { status }));
}

afterEach(() => { vi.unstubAllGlobals(); });

// Stage 4 review 2026-10-29: consolidation/reasoning ignored the ingress result, so a 403,
// a 400 or Letta's ok:false all looked like success.
describe("postMemoryWrite", () => {
  it("is ok only for 2xx with ok:true, and sends the secret header and an append to human", async () => {
    const f = respond(200, { ok: true });
    vi.stubGlobal("fetch", f);
    expect(await postMemoryWrite("hello", "t-1", "s3")).toEqual({ ok: true });
    const [url, init] = f.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("http://localhost:5678/webhook/terrance/ingress");
    expect((init.headers as Record<string, string>)["X-Nexus-Secret"]).toBe("s3");
    expect(JSON.parse(String(init.body))).toEqual({ trace_id: "t-1", event: { type: "memory.write", payload: { content: "hello", mode: "append", block: "human" } } });
  });

  it.each([
    [403, "", 403],
    [400, { ok: false, error: { message: "content exceeds 8000 characters" } }, 400],
    [200, { ok: false, error: { message: "memory.write failed", status: 500 } }, 200],
    [200, "not json", 200],
  ])("HTTP %s %j -> failed with status", async (status, body, expected) => {
    vi.stubGlobal("fetch", respond(status, body));
    expect(await postMemoryWrite("x", "t", "s")).toEqual({ ok: false, status: expected });
  });

  it("reports n8n unreachable when fetch throws", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new TypeError("Load failed"); }));
    const r = await postMemoryWrite("x", "t");
    expect(r).toEqual({ ok: false, status: null });
    expect(memoryWriteFailure(r)).toBe("Letta write failed (n8n unreachable)");
    expect(memoryWriteFailure({ ok: false, status: 403 })).toBe("Letta write failed (HTTP 403)");
    expect(memoryWriteFailure({ ok: true })).toBeNull();
  });

  it("caps content at the ingress limit and omits the header without a secret", async () => {
    const f = respond(200, { ok: true });
    vi.stubGlobal("fetch", f);
    await postMemoryWrite("a".repeat(MEMORY_WRITE_MAX_CHARS + 500), "t");
    const init = (f.mock.calls[0] as unknown as [string, RequestInit])[1];
    const sent = JSON.parse(String(init.body)).event.payload.content as string;
    expect(sent.length).toBe(MEMORY_WRITE_MAX_CHARS);
    expect(sent.endsWith("…")).toBe(true);
    expect((init.headers as Record<string, string>)["X-Nexus-Secret"]).toBeUndefined();
  });
});

// 1b6b2f4: content is capped client-side so the ingress (MW Prepare rejects > 8000) accepts it.
describe("postMemoryWrite 8000-char boundary", () => {
  const sent = async (content: string) => {
    const f = respond(200, { ok: true });
    vi.stubGlobal("fetch", f);
    await postMemoryWrite(content, "t");
    const [, init] = f.mock.calls[0] as unknown as [string, RequestInit];
    return JSON.parse(String(init.body)).event.payload.content as string;
  };

  it("8000 chars are sent whole", async () => {
    const content = `${"a".repeat(7999)}Z`;
    expect(MEMORY_WRITE_MAX_CHARS).toBe(8000);
    expect(await sent(content)).toBe(content);
  });

  it("8001 chars: the head is kept and the tail cut to 7999 + '…' = 8000", async () => {
    const out = await sent(`HEAD${"a".repeat(7996)}TAIL`.slice(0, 8001));
    expect(out).toHaveLength(8000);
    expect(out.startsWith("HEAD")).toBe(true);
    expect(out.endsWith("…")).toBe(true);
  });
});
