import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// Read-only: the exported Ingress (stack/n8n/workflows/eYdwlwZKTMV89z2J.json), never n8n itself.
// The crew key (CREW_INGRESS_SECRET, credential nxCrewIngress1) authenticates only the
// "Crew Ingress Webhook", whose router sends kb.query and memory.query to the existing read
// handlers and everything else to a 403. On the main webhook the crew key fails Header Auth
// (credential nxIngressSecret1), which n8n answers with 403.
type Node = {
  name: string; type: string;
  parameters: Record<string, any>;
  credentials?: Record<string, { id: string; name: string }>;
};
type Conn = Record<string, { main: { node: string }[][] }>;
const wf = JSON.parse(readFileSync(resolve(__dirname, "../../stack/n8n/workflows/eYdwlwZKTMV89z2J.json"), "utf8")) as {
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

  it("the crew webhook uses its own Header Auth credential; the main webhook keeps the Nexus one", () => {
    expect(wf.id).toBe("eYdwlwZKTMV89z2J");
    expect(crew.parameters).toMatchObject({ authentication: "headerAuth", httpMethod: "POST", path: "terrance/ingress-crew", responseMode: "responseNode" });
    expect(crew.credentials?.httpHeaderAuth.id).toBe("nxCrewIngress1");
    expect(main.parameters.authentication).toBe("headerAuth");
    expect(main.credentials?.httpHeaderAuth.id).toBe("nxIngressSecret1");
    expect(crew.parameters.path).not.toBe(main.parameters.path);
  });

  it.each([["kb.query", "Handle kb.query"], ["memory.query", "MQ Prepare"]])("crew key may call %s", (type, handler) => {
    expect(target("Crew Route (read-only)", type)).toEqual([handler]);
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
    for (const n of ["Handle kb.query", "MQ Prepare", "MQ Letta GET", "Send Response", "Reject Crew Forbidden"]) {
      expect(r.has(n), n).toBe(true);
    }
  });

  it("the main webhook's routes are unchanged (crew key fails its Header Auth: 403 by n8n)", () => {
    expect(target("Route by event.type", "memory.write")).toEqual(["MW Prepare"]);
    expect(target("Route by event.type", "kb.ingest")).toEqual(["Handle kb.ingest"]);
    expect(reachable("Ingress Webhook").has("Crew Route (read-only)")).toBe(false);
  });
});

describe("n8n_sync_credentials.sh creates the crew credential the workflow references", () => {
  const sh = readFileSync(resolve(__dirname, "../../stack/scripts/n8n_sync_credentials.sh"), "utf8");

  it("nxCrewIngress1 from CREW_INGRESS_SECRET, refused if it equals NEXUS_SECRET, skipped if unset", () => {
    expect(sh).toContain('"id": "nxCrewIngress1"');
    expect(sh).toContain("env.get('CREW_INGRESS_SECRET')");
    expect(sh).toContain("crew == env['NEXUS_SECRET']");
    expect(sh).toMatch(/CREW_INGRESS_SECRET is empty in \.env; skipping/);
  });
});
