import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { INGRESS_WORKFLOW_PATH } from "./workflowPath";

// Read-only: the exported Ingress (INGRESS_WORKFLOW_PATH), never n8n itself.
// The crew key (CREW_INGRESS_SECRET) is checked by the "Crew Auth" node against its SHA-256
// (rendered in by n8n_render_crew_auth.sh): missing or wrong is 401. Past that, the router sends
// kb.query and memory.query to the existing read handlers and everything else to a 403. On the
// main webhook the crew key fails Header Auth (credential nxIngressSecret1), which n8n answers 403.
type Node = {
  name: string; type: string;
  parameters: Record<string, any>;
  credentials?: Record<string, { id: string; name: string }>;
};
type Conn = Record<string, { main: { node: string }[][] }>;
const wf = JSON.parse(readFileSync(INGRESS_WORKFLOW_PATH, "utf8")) as {
  id: string; nodes: Node[]; connections: Conn;
};
const node = (name: string) => {
  const n = wf.nodes.find((x) => x.name === name);
  if (!n) throw new Error(`no node ${name}`);
  return n;
};

/** Every node reachable from `start` along main connections. */
function reachable(start: string): Set<string> {
  const seen = new Set<string>([start]);
  const queue = [start];
  while (queue.length) {
    const cur = queue.shift()!;
    for (const out of wf.connections[cur]?.main ?? []) {
      for (const { node: next } of out) if (!seen.has(next)) { seen.add(next); queue.push(next); }
    }
  }
  return seen;
}

/** Evaluate a Switch v3 "rules" node on $json.body.event.type: the output index taken. */
function route(switchName: string, eventType: unknown): number {
  const sw = node(switchName);
  const rules = sw.parameters.rules.values as { conditions: { conditions: { leftValue: string; rightValue: string; operator: { operation: string } }[] } }[];
  for (const [i, r] of rules.entries()) {
    const ok = r.conditions.conditions.every((c) => {
      expect(c.leftValue).toBe("={{ $json.body.event.type }}");
      expect(c.operator.operation).toBe("equals");
      return eventType === c.rightValue;
    });
    if (ok) return i;
  }
  expect(sw.parameters.options?.fallbackOutput).toBe("extra");
  return rules.length;
}
const target = (switchName: string, eventType: unknown) =>
  wf.connections[switchName].main[route(switchName, eventType)].map((c) => c.node);

/** Evaluate a respondToWebhook body expression `={{ ... }}` against an incoming $json. */
function respond(name: string, json: Record<string, unknown>) {
  const p = node(name).parameters;
  const expr = String(p.responseBody).replace(/^=\{\{\s*/, "").replace(/\s*\}\}$/, "");
  return { code: p.options?.responseCode ?? 200, body: new Function("$json", `return (${expr});`)(json) };
}

describe("crew key: separate, read-only webhook", () => {
  const crew = node("Crew Ingress Webhook");
  const main = node("Ingress Webhook");

  it("the crew webhook checks its key in Crew Auth; the main webhook keeps the Nexus credential", () => {
    expect(wf.id).toBe("eYdwlwZKTMV89z2J");
    expect(crew.parameters).toMatchObject({ authentication: "none", httpMethod: "POST", path: "terrance/ingress-crew", responseMode: "responseNode" });
    expect(crew.credentials).toBeUndefined();
    expect(wf.connections["Crew Ingress Webhook"].main[0].map((c) => c.node)).toEqual(["Crew Auth"]);
    expect(wf.connections["Crew Auth"].main[0].map((c) => c.node)).toEqual(["Crew Authorized?"]);
    expect(wf.connections["Crew Authorized?"].main.map((o) => o.map((c) => c.node))).toEqual([["Crew Route (read-only)"], ["Reject Crew Unauthorized"]]);
    expect(main.parameters.authentication).toBe("headerAuth");
    expect(main.credentials?.httpHeaderAuth.id).toBe("nxIngressSecret1");
    expect(crew.parameters.path).not.toBe(main.parameters.path);
  });

  it.each([["kb.query", "Crew KB Sanitize"], ["memory.query", "MQ Prepare"]])("crew key may call %s", (type, handler) => {
    expect(target("Crew Route (read-only)", type)).toEqual([handler]);
  });

  it("crew kb.query: Crew KB Sanitize → Handle kb.query, forwarding only query/labels/document_filter", () => {
    expect(wf.connections["Crew KB Sanitize"].main[0].map((c) => c.node)).toEqual(["Handle kb.query"]);
    const code = node("Crew KB Sanitize").parameters.jsCode as string;
    const run = new Function("$input", code) as (i: unknown) => { json: { body: any } }[];
    const out = (body: unknown) => run({ first: () => ({ json: { body } }) })[0].json.body;
    const evil = { trace_id: "t", event: { type: "kb.query", payload: {
      query: "nexus", labels: ["a", 5], document_filter: ["d"],
      rag_config: { Retriever: { components: { url: "http://169.254.169.254/" } } }, extra: 1,
    } } };
    expect(out(evil)).toEqual({ trace_id: "t", event: { type: "kb.query", payload: { query: "nexus", labels: ["a"], document_filter: ["d"] } } });
    expect(JSON.stringify(out(evil))).not.toContain("169.254");
    expect(out({}).event.payload).toEqual({ query: "", labels: [], document_filter: [] });
    expect(out({ event: { payload: { query: "q".repeat(5000) } } }).event.payload.query).toHaveLength(2000);
  });

  it.each(["memory.write", "kb.ingest", "health.ping", "admin.reset", "workflow.activate", "KB.QUERY", "", undefined, 42])(
    "crew key gets 403 for %s",
    (type) => {
      expect(target("Crew Route (read-only)", type)).toEqual(["Reject Crew Forbidden"]);
      const r = respond("Reject Crew Forbidden", { body: { trace_id: "t1", event: { type } } });
      expect(r.code).toBe(403);
      expect(r.body).toMatchObject({ ok: false, trace_id: "t1", error: { node: "crew-router" } });
    },
  );

  it("the 403 works without a body or trace_id", () => {
    expect(respond("Reject Crew Forbidden", {}).body).toMatchObject({ ok: false, trace_id: "no-trace-id" });
  });

  it("no write or admin node is reachable from the crew webhook", () => {
    const r = reachable("Crew Ingress Webhook");
    for (const n of ["MW Prepare", "MW Letta PATCH", "Handle kb.ingest", "Handle health.ping", "Handle unknown", "Route by event.type"]) {
      expect(r.has(n), n).toBe(false);
    }
    for (const n of ["Crew Auth", "Crew Authorized?", "Reject Crew Unauthorized", "Crew KB Sanitize", "Handle kb.query", "MQ Prepare", "MQ Letta GET", "Send Response", "Reject Crew Forbidden"]) {
      expect(r.has(n), n).toBe(true);
    }
  });

  it("the main webhook's routes are unchanged (crew key fails its Header Auth: 403 by n8n)", () => {
    expect(target("Route by event.type", "memory.write")).toEqual(["MW Prepare"]);
    expect(target("Route by event.type", "kb.ingest")).toEqual(["Handle kb.ingest"]);
    expect(reachable("Ingress Webhook").has("Crew Route (read-only)")).toBe(false);
  });
});

describe("Crew Auth: constant-time key check, 401 on a missing or wrong key", () => {
  const KEY = randomBytes(48).toString("base64url");
  const sha = (v: string) => createHash("sha256").update(v).digest("hex");
  const code = node("Crew Auth").parameters.jsCode as string;
  const auth = (headers: Record<string, unknown>, expected = sha(KEY)) => {
    const run = new Function("$input", code.replace("__CREW_KEY_SHA256__", expected)) as (i: unknown) => { json: any }[];
    return run({ first: () => ({ json: { headers, body: { trace_id: "t", event: { type: "kb.query" } } } }) })[0].json;
  };

  it("the committed export holds only the placeholder, never a key or hash", () => {
    expect(code).toContain("const EXPECTED = '__CREW_KEY_SHA256__';");
    expect(code.match(/[0-9a-f]{64}/)).toBeNull();
  });

  it("the right key is authorized and the header is dropped", () => {
    const out = auth({ "x-nexus-secret": KEY });
    expect(out).toEqual({ authorized: true, body: { trace_id: "t", event: { type: "kb.query" } } });
    expect(JSON.stringify(out)).not.toContain(KEY);
  });

  it.each([
    ["missing", {}],
    ["empty", { "x-nexus-secret": "" }],
    ["wrong", { "x-nexus-secret": randomBytes(48).toString("base64url") }],
    ["prefix", { "x-nexus-secret": KEY.slice(0, -1) }],
    ["array", { "x-nexus-secret": [KEY] }],
    ["the hash itself", { "x-nexus-secret": sha(KEY) }],
  ])("%s key is not authorized", (_label, headers) => {
    expect(auth(headers)).toEqual({ authorized: false, body: expect.any(Object) });
  });

  it("an unrendered placeholder never matches (fail closed)", () => {
    expect(auth({ "x-nexus-secret": KEY }, "__CREW_KEY_SHA256__").authorized).toBe(false);
    expect(auth({ "x-nexus-secret": "__CREW_KEY_SHA256__" }, "__CREW_KEY_SHA256__").authorized).toBe(false);
  });

  it("the compare is constant-time: a fixed 64-step XOR fold, no early return or ===", () => {
    const fn = code.slice(code.indexOf("function safeEqual"), code.indexOf("const item"));
    expect(fn).toMatch(/for \(let i = 0; i < 64; i\+\+\) diff \|=/);
    expect(fn).not.toMatch(/return false|===\s*b|a\s*===/);
    expect(code).not.toMatch(/sha256hex\(got\)\s*===/);
  });

  it("the SHA-256 matches node:crypto across padding boundaries", () => {
    for (const len of [1, 55, 56, 63, 64, 65, 119, 120, 512]) {
      const k = "k".repeat(len);
      expect(auth({ "x-nexus-secret": k }, sha(k)).authorized, String(len)).toBe(true);
    }
  });

  it("bad key: 401 with a fixed body that never echoes the header", () => {
    const r = respond("Reject Crew Unauthorized", { authorized: false, body: { trace_id: "t9" } });
    expect(r.code).toBe(401);
    expect(r.body).toEqual({ ok: false, trace_id: "no-trace-id", error: { message: "Unauthorized", node: "crew-auth" } });
    expect(String(node("Reject Crew Unauthorized").parameters.responseBody)).not.toMatch(/headers|secret/i);
  });

  it("a valid key on memory.write: authorized, then 403 from the router", () => {
    expect(auth({ "x-nexus-secret": KEY }).authorized).toBe(true);
    expect(target("Crew Route (read-only)", "memory.write")).toEqual(["Reject Crew Forbidden"]);
    expect(respond("Reject Crew Forbidden", { body: { event: { type: "memory.write" } } }).code).toBe(403);
  });
});

const REPO = resolve(__dirname, "../..");
function withTmp<T>(fn: (dir: string) => T): T {
  const dir = mkdtempSync(join(tmpdir(), "crew-key-"));
  try { return fn(dir); } finally { rmSync(dir, { recursive: true, force: true }); }
}
function run(script: string, args: string[]) {
  const r = spawnSync(script, args, { encoding: "utf8" });
  return { code: r.status, out: `${r.stdout}${r.stderr}` };
}

describe("n8n_render_crew_auth.sh renders the hash; n8n_sync_credentials.sh has no crew credential", () => {
  const render = join(REPO, "stack/scripts/n8n_render_crew_auth.sh");
  const exportPath = INGRESS_WORKFLOW_PATH;
  const KEY = randomBytes(48).toString("base64url");
  const NEXUS = randomBytes(48).toString("base64url");
  const renderWith = (dir: string, env: string) => {
    writeFileSync(join(dir, ".env"), env);
    const file = join(dir, "rendered.json");
    return { ...run(render, ["--env", join(dir, ".env"), "--in", exportPath, "--out", file]), file };
  };

  it("fills in the key's SHA-256, writes 0600 and prints neither key nor hash", () => withTmp((dir) => {
    const r = renderWith(dir, `NEXUS_SECRET=${NEXUS}\nCREW_INGRESS_SECRET=${KEY}\n`);
    expect(r.code).toBe(0);
    const hash = createHash("sha256").update(KEY).digest("hex");
    expect(readFileSync(r.file, "utf8")).toContain(`const EXPECTED = '${hash}';`);
    expect(statSync(r.file).mode & 0o777).toBe(0o600);
    expect(r.out).not.toContain(KEY);
    expect(r.out).not.toContain(hash);
  }));

  it("an empty key revokes: a random hash, never the hash of an empty or known value", () => withTmp((dir) => {
    const r = renderWith(dir, `NEXUS_SECRET=${NEXUS}\nCREW_INGRESS_SECRET=\n`);
    expect(r.code).toBe(0);
    expect(r.out).toContain("revoked");
    const body = readFileSync(r.file, "utf8");
    expect(body).not.toContain("__CREW_KEY_SHA256__");
    for (const v of ["", NEXUS]) expect(body).not.toContain(createHash("sha256").update(v).digest("hex"));
  }));

  it.each([
    ["equal to NEXUS_SECRET", (n: string) => `NEXUS_SECRET=${n}\nCREW_INGRESS_SECRET=${n}\n`, "must differ"],
    ["shorter than 32 bytes", (n: string) => `NEXUS_SECRET=${n}\nCREW_INGRESS_SECRET=abc\n`, "shorter than 43"],
  ])("refuses a key %s", (_l, env, msg) => withTmp((dir) => {
    const r = renderWith(dir, env(NEXUS));
    expect(r.code).not.toBe(0);
    expect(r.out).toContain(msg);
    expect(r.out).not.toContain(NEXUS);
  }));

  it("the main credentials stay; the unused crew credential is gone", () => {
    const sh = readFileSync(join(REPO, "stack/scripts/n8n_sync_credentials.sh"), "utf8");
    expect(sh).toContain('"id": "nxIngressSecret1"');
    expect(sh).not.toContain('"id": "nxCrewIngress1"');
  });

  it("the rendered output directory is git-ignored", () => {
    expect(readFileSync(join(REPO, ".gitignore"), "utf8")).toMatch(/^stack\/n8n\/rendered\/$/m);
  });
});

describe("sync-crew-env.sh: own 32+ byte key, 0600, revoke on clear", () => {
  const sync = join(REPO, "scripts/sync-crew-env.sh");
  const NEXUS = randomBytes(48).toString("base64url");
  const syncWith = (dir: string, env: string) => {
    writeFileSync(join(dir, ".env"), env);
    return run(sync, ["--env", join(dir, ".env"), "--dest", join(dir, ".env.crew")]);
  };

  it("writes only CREW_INGRESS_SECRET, mode 0600, without printing it", () => withTmp((dir) => {
    const key = randomBytes(48).toString("base64url");
    const r = syncWith(dir, `NEXUS_SECRET=${NEXUS}\nCREW_INGRESS_SECRET=${key}\n`);
    expect(r).toEqual({ code: 0, out: "written\n" });
    const body = readFileSync(join(dir, ".env.crew"), "utf8");
    expect(body).toContain(`CREW_INGRESS_SECRET=${key}\n`);
    expect(body).not.toContain(NEXUS);
    expect(statSync(join(dir, ".env.crew")).mode & 0o777).toBe(0o600);
  }));

  it.each([
    ["reuses NEXUS_SECRET", (n: string) => `NEXUS_SECRET=${n}\nCREW_INGRESS_SECRET=${n}\n`, "must differ"],
    ["is under 32 bytes", (n: string) => `NEXUS_SECRET=${n}\nCREW_INGRESS_SECRET=${"a".repeat(42)}\n`, "shorter than 43"],
  ])("refuses a key that %s", (_l, env, msg) => withTmp((dir) => {
    const r = syncWith(dir, env(NEXUS));
    expect(r.code).toBe(1);
    expect(r.out).toContain(msg);
    expect(r.out).not.toContain(NEXUS);
  }));

  it("an emptied key overwrites the old one (revoked)", () => withTmp((dir) => {
    syncWith(dir, `CREW_INGRESS_SECRET=${randomBytes(48).toString("base64url")}\n`);
    const r = syncWith(dir, "CREW_INGRESS_SECRET=\n");
    expect(r).toEqual({ code: 0, out: "cleared (crew key revoked)\n" });
    expect(readFileSync(join(dir, ".env.crew"), "utf8")).toMatch(/\nCREW_INGRESS_SECRET=\n$/);
  }));

  it("copies the non-secret CREW_ALLOWED_TOOLS (for the app's crew_tools) and validates it", () => withTmp((dir) => {
    const key = randomBytes(48).toString("base64url");
    expect(syncWith(dir, `NEXUS_SECRET=${NEXUS}\nCREW_INGRESS_SECRET=${key}\nCREW_ALLOWED_TOOLS=kb_query,memory_query\n`).code).toBe(0);
    expect(readFileSync(join(dir, ".env.crew"), "utf8")).toContain("\nCREW_ALLOWED_TOOLS=kb_query,memory_query\n");
    const bad = syncWith(dir, `NEXUS_SECRET=${NEXUS}\nCREW_INGRESS_SECRET=${key}\nCREW_ALLOWED_TOOLS=kb_query;rm -rf\n`);
    expect(bad.code).toBe(1);
    expect(bad.out).toContain("CREW_ALLOWED_TOOLS may only hold tool names");
  }));

  it("writes crew-tools.conf next to --dest: only CREW_ALLOWED_TOOLS, mode 0600, no key", () => withTmp((dir) => {
    const key = randomBytes(48).toString("base64url");
    expect(syncWith(dir, `NEXUS_SECRET=${NEXUS}\nCREW_INGRESS_SECRET=${key}\nCREW_ALLOWED_TOOLS=kb_query\n`).code).toBe(0);
    const conf = join(dir, "crew-tools.conf");
    expect(readFileSync(conf, "utf8")).toBe("CREW_ALLOWED_TOOLS=kb_query\n");
    expect(statSync(conf).mode & 0o777).toBe(0o600);
  }));

  it("stack/crew-tools.conf is git-ignored", () => {
    expect(execFileSync("git", ["check-ignore", "stack/crew-tools.conf"], { cwd: REPO, encoding: "utf8" }).trim()).toBe("stack/crew-tools.conf");
  });

  it("stack/.env.crew is git-ignored and untracked", () => {
    expect(execFileSync("git", ["check-ignore", "stack/.env.crew"], { cwd: REPO, encoding: "utf8" }).trim()).toBe("stack/.env.crew");
    expect(execFileSync("git", ["ls-files", "stack/.env.crew"], { cwd: REPO, encoding: "utf8" })).toBe("");
  });
});
