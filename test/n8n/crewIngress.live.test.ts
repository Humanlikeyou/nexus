import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// Live crew webhook on the local n8n. Skipped unless NEXUS_LIVE_CREW=1; needs the rendered
// export imported (stack/scripts/n8n_render_crew_auth.sh). The key is read from stack/.env.crew
// in-process and never printed or put in an assertion message.
const LIVE = process.env.NEXUS_LIVE_CREW === "1";
const URL = process.env.CREW_INGRESS_LIVE_URL ?? "http://127.0.0.1:5678/webhook/terrance/ingress-crew";

function crewKey(): string {
  const env = readFileSync(resolve(__dirname, "../../stack/.env.crew"), "utf8");
  const m = env.match(/^CREW_INGRESS_SECRET=(.*)$/m);
  return (m?.[1] ?? "").trim();
}
async function call(type: string, key: string | null, payload: Record<string, unknown> = {}) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (key !== null) headers["X-Nexus-Secret"] = key;
  const res = await fetch(URL, { method: "POST", headers, body: JSON.stringify({ trace_id: "live-test", event: { type, payload } }) });
  return { status: res.status, text: await res.text() };
}

describe.skipIf(!LIVE)("live crew webhook (NEXUS_LIVE_CREW=1)", () => {
  const key = LIVE ? crewKey() : "";

  it("has a crew key to test with", () => expect(key.length >= 43).toBe(true));

  it("kb.query: 200, no raw Verba/gRPC text", async () => {
    const r = await call("kb.query", key, { query: "nexus" });
    expect(r.status).toBe(200);
    for (const raw of ["GRPC", "AioRpcError", "debug_error_string", "Traceback"]) expect(r.text.includes(raw)).toBe(false);
  });

  it("memory.query: 200", async () => expect((await call("memory.query", key)).status).toBe(200));

  it("memory.write: 403", async () => expect((await call("memory.write", key, { label: "x", value: "y" })).status).toBe(403));

  it.each([["no key", null], ["bad key", "not-the-crew-key-000000000000000000000000000"]])("%s: 401, header not echoed", async (_l, bad) => {
    const r = await call("kb.query", bad, { query: "nexus" });
    expect(r.status).toBe(401);
    expect(r.text.includes(key)).toBe(false);
  });
});
