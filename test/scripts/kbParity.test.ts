import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, describe, expect, it } from "vitest";
// @ts-expect-error plain .mjs script without types
import { QUESTIONS, compare, isLoopback, snapshot, topKIds } from "../../tasks/proposed/kb-parity.mjs";

const KEY = "SENTINEL-parity-key-4c1b-do-not-leak";
const CHUNKS = Array.from({ length: 12 }, (_, i) => ({
  id: `00000000-0000-0000-0000-${String(i).padStart(12, "0")}`,
  text: ["restart stack docker", "crew tool grant approval", "muse recordings folder", "letta password keychain", "outbound switch off", "stack status health"][i % 6] + ` ${i}`,
}));
const servers: Server[] = [];
afterEach(() => { for (const s of servers.splice(0)) s.close(); });

/** Fixture Weaviate: BM25-ish token overlap, ties broken by id. `requireKey` mimics anonymous=false. */
async function fixture(requireKey?: string): Promise<string> {
  const s = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      if (requireKey && req.headers.authorization !== `Bearer ${requireKey}`) { res.writeHead(401).end(`{"error":"unauthorized"}`); return; }
      const q: string = JSON.parse(body).query;
      const words = new Set((/query: "(.*?)"/.exec(q)?.[1] ?? "").toLowerCase().split(/\W+/).filter(Boolean));
      const scored = CHUNKS.map((c) => ({ c, n: c.text.split(/\W+/).filter((w) => words.has(w)).length }))
        .filter((x) => x.n > 0).sort((a, b) => b.n - a.n || a.c.id.localeCompare(b.c.id)).slice(0, 5);
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ data: { Get: { VERBA_Embedding_nomic_embed_text: scored.map((x) => ({ _additional: { id: x.c.id } })) } } }));
    });
  });
  await new Promise<void>((r) => s.listen(0, "127.0.0.1", r));
  servers.push(s);
  return `http://127.0.0.1:${(s.address() as AddressInfo).port}`;
}

describe("kb parity harness (fixture only, never the live stack)", () => {
  it("anonymous-before and keyed-after snapshots are identical for the fixed 5 questions", async () => {
    const before = await snapshot(await fixture(), undefined);
    const after = await snapshot(await fixture(KEY), KEY);
    expect(Object.keys(before)).toEqual(QUESTIONS);
    expect(Object.values(before).some((ids) => (ids as string[]).length > 0)).toBe(true);
    expect(compare(before, after)).toEqual([]);
    expect(JSON.stringify(after)).not.toContain("SENTINEL");
  });
  it("detects a changed top-k", async () => {
    const before = await snapshot(await fixture(), undefined);
    const after = JSON.parse(JSON.stringify(before));
    after[QUESTIONS[0]].reverse();
    expect(compare(before, after).length).toBe(1);
  });
  it("anonymous access to an auth-required server fails without echoing the body", async () => {
    const base = await fixture(KEY);
    await expect(topKIds(base, undefined, QUESTIONS[0])).rejects.toThrow("weaviate HTTP 401");
    await expect(topKIds(base, "wrong", QUESTIONS[0])).rejects.not.toThrow(/unauthorized|wrong/);
  });
  it("refuses a non-loopback base, so the key never leaves this Mac", async () => {
    for (const b of ["http://example.com:8080", "http://127.0.0.1@evil.example", "https://127.0.0.1"]) {
      expect(isLoopback(b)).toBe(false);
      await expect(topKIds(b, KEY, "x")).rejects.toThrow("loopback");
    }
  });
});
