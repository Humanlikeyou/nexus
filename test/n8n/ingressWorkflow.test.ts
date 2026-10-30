import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { INGRESS_WORKFLOW_PATH } from "./workflowPath";

// Read-only: the Ingress export (INGRESS_WORKFLOW_PATH), never n8n itself. Each Code node's jsCode is
// run with new Function on a fake $input / $('Node'), the same globals n8n gives it.
const workflow = JSON.parse(readFileSync(INGRESS_WORKFLOW_PATH, "utf8")) as {
  nodes: { name: string; type: string; parameters: { jsCode?: string } }[];
};
type Item = { json: Record<string, unknown> };
function codeNode(name: string) {
  const node = workflow.nodes.find((n) => n.name === name);
  if (!node?.parameters.jsCode) throw new Error(`${name}: no jsCode`);
  const run = new Function("$input", "$", node.parameters.jsCode) as (input: unknown, $: unknown) => Item[];
  return (input: Record<string, unknown>, nodes: Record<string, Record<string, unknown>> = {}) =>
    run({ first: () => ({ json: input }) }, (n: string) => ({ first: () => ({ json: nodes[n] }) }))[0].json;
}

describe("MW Compose (2e07a8a): the Letta block stays ≤ 15000 chars, newest lines kept", () => {
  const compose = codeNode("MW Compose");
  const prep = { trace_id: "t", block: "human", mode: "append", patch_url: "http://letta/x", content: "NEWEST entry" };

  it("30k of existing memory: trimmed from the oldest lines, the new entry kept", () => {
    const old = Array.from({ length: 3000 }, (_, i) => `line ${String(i).padStart(4, "0")} ${"x".repeat(1)}`).join("\n");
    expect(old.length).toBeGreaterThan(30_000);
    const out = compose({ statusCode: 200, body: { blocks: [{ label: "human", value: old }] } }, { "MW Prepare": prep });
    const value = out.value as string;
    expect(out.proceed).toBe(true);
    expect(value.length).toBeLessThanOrEqual(15_000);
    expect(value.endsWith("\nNEWEST entry")).toBe(true);
    expect(value).toContain("line 2999");
    expect(value).not.toContain("line 0000");
    expect(value.startsWith("line ")).toBe(true); // cut on a line boundary
    expect(out.trimmed).toBeGreaterThan(0);
  });

  it("one oversized line keeps its newest 15000-char tail", () => {
    const out = compose({ statusCode: 200, body: { blocks: [{ label: "human", value: "" }] } }, { "MW Prepare": { ...prep, content: `${"a".repeat(20_000)}TAIL` } });
    expect((out.value as string).length).toBe(15_000);
    expect((out.value as string).endsWith("TAIL")).toBe(true);
  });

  it("under the limit: untouched", () => {
    const out = compose({ statusCode: 200, body: { blocks: [{ label: "human", value: "a\nb" }] } }, { "MW Prepare": prep });
    expect(out).toMatchObject({ value: "a\nb\nNEWEST entry", trimmed: 0 });
  });

  it("a failed Letta read does not proceed", () => {
    expect(compose({ statusCode: 500 }, { "MW Prepare": prep })).toMatchObject({ proceed: false, ok: false });
  });
});

describe("MW Prepare: server-side 8000-char cap (matches MEMORY_WRITE_MAX_CHARS)", () => {
  const prepare = codeNode("MW Prepare");
  const body = (content: string) => ({ body: { trace_id: "t", event: { type: "memory.write", payload: { content } } } });

  it("8000 accepted, 8001 rejected", () => {
    expect(prepare(body("x".repeat(8000)))).toMatchObject({ valid: true });
    expect(prepare(body("x".repeat(8001)))).toMatchObject({ valid: false, error: { message: "content exceeds 8000 characters" } });
  });
});
