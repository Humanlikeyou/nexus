#!/usr/bin/env node
// Weaviate switch-over parity harness (tasks/proposed/weaviate-switchover.md, step 3).
//
//   snapshot: ask Weaviate the fixed 5 questions (BM25 on chunk text, top-k) and save the ids.
//   compare : diff two snapshots (before / after the auth switch). Exit 1 on any difference.
//
// LIVE USE IS "NOT RUN": nothing here has been pointed at the real stack. The automated test
// (test/scripts/kbParity.test.ts) runs only against an in-process fixture server.
// BM25 is deterministic and needs no embedding call; it proves the DATA and the query path are
// unchanged, not vector ranking (Verba's hybrid query is not exercised).
//
//   node tasks/proposed/kb-parity.mjs snapshot --out /tmp/before.json
//   KB_PARITY_KEY=... node tasks/proposed/kb-parity.mjs snapshot --out /tmp/after.json --key-env KB_PARITY_KEY
//   node tasks/proposed/kb-parity.mjs compare /tmp/before.json /tmp/after.json
//
// The key is read from the environment variable NAMED by --key-env (never from argv), sent only as
// `Authorization: Bearer` to a loopback base, and never written to the snapshot or to output.
import { readFileSync, writeFileSync } from "node:fs";

export const QUESTIONS = [
  "How do I restart the local stack?",
  "What does the crew service do with a tool grant?",
  "Where are the Muse recordings stored?",
  "How is the Letta memory password kept?",
  "What changes when the outbound switch is off?",
];
export const CHUNK_CLASS = "VERBA_Embedding_nomic_embed_text";
export const TOP_K = 5;

export function isLoopback(base) {
  try {
    const u = new URL(base);
    const h = u.hostname.replace(/^\[|\]$/g, "");
    return u.protocol === "http:" && !u.username && !u.password && (h === "localhost" || h === "127.0.0.1" || h === "::1");
  } catch { return false; }
}

export async function topKIds(base, key, question, k = TOP_K) {
  if (!isLoopback(base)) throw new Error("base must be a loopback http URL");
  const q = JSON.stringify(question);
  const gql = `{ Get { ${CHUNK_CLASS}(bm25: {query: ${q}, properties: ["text"]}, limit: ${k}) { _additional { id } } } }`;
  const headers = { "Content-Type": "application/json" };
  if (key) headers.Authorization = `Bearer ${key}`;
  const r = await fetch(`${base}/v1/graphql`, { method: "POST", headers, body: JSON.stringify({ query: gql }), redirect: "error", signal: AbortSignal.timeout(10_000) });
  if (!r.ok) throw new Error(`weaviate HTTP ${r.status}`); // never echo the body
  const j = await r.json();
  if (j.errors) throw new Error("weaviate returned errors");
  return (j.data?.Get?.[CHUNK_CLASS] ?? []).map((o) => o._additional.id);
}

export async function snapshot(base, key) {
  const out = {};
  for (const q of QUESTIONS) out[q] = await topKIds(base, key, q);
  return out;
}

/** [] when identical; otherwise one line per differing question (ids only). */
export function compare(before, after) {
  const diffs = [];
  for (const q of QUESTIONS) {
    const a = JSON.stringify(before[q] ?? null), b = JSON.stringify(after[q] ?? null);
    if (a !== b) diffs.push(`${q}\n  before ${a}\n  after  ${b}`);
  }
  return diffs;
}

async function main(argv) {
  const [cmd, ...rest] = argv;
  const opt = (n) => { const i = rest.indexOf(n); return i >= 0 ? rest[i + 1] : undefined; };
  const base = opt("--base") ?? "http://127.0.0.1:8080";
  if (cmd === "snapshot") {
    const keyEnv = opt("--key-env");
    const key = keyEnv ? process.env[keyEnv] : undefined;
    if (keyEnv && !key) { console.error(`env ${keyEnv} is empty`); return 2; }
    const snap = await snapshot(base, key);
    writeFileSync(opt("--out") ?? "/dev/stdout", JSON.stringify(snap, null, 2) + "\n");
    return 0;
  }
  if (cmd === "compare") {
    const d = compare(JSON.parse(readFileSync(rest[0], "utf8")), JSON.parse(readFileSync(rest[1], "utf8")));
    console.log(d.length ? `PARITY FAIL\n${d.join("\n")}` : "PARITY OK (5/5 identical)");
    return d.length ? 1 : 0;
  }
  console.error("usage: snapshot [--base URL] [--key-env NAME] [--out FILE] | compare A.json B.json");
  return 2;
}
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  main(process.argv.slice(2)).then((c) => process.exit(c), (e) => { console.error(String(e.message ?? e)); process.exit(2); });
}
