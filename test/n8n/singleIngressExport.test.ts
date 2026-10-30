import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { INGRESS_WORKFLOW_ID, INGRESS_WORKFLOW_REL } from "./workflowPath";

// Exactly one repo .json may hold the Ingress workflow body: the export at INGRESS_WORKFLOW_REL.
// A body is a workflow with id eYdwlwZKTMV89z2J, or one whose nodes include the main
// terrance/ingress webhook (old copies were exported without an id).
const REPO = resolve(__dirname, "../..");

function holdsIngressBody(doc: unknown): boolean {
  if (!doc || typeof doc !== "object") return false;
  const d = doc as { id?: unknown; nodes?: unknown; activeVersion?: unknown };
  if (d.id === INGRESS_WORKFLOW_ID) return true;
  const nodes = Array.isArray(d.nodes) ? d.nodes : [];
  if (nodes.some((n: any) => n?.type === "n8n-nodes-base.webhook" && n?.parameters?.path === "terrance/ingress")) return true;
  return holdsIngressBody(d.activeVersion);
}

describe("one Ingress export", () => {
  it(`only ${INGRESS_WORKFLOW_REL} holds the workflow body for ${INGRESS_WORKFLOW_ID}`, () => {
    const files = execFileSync("git", ["ls-files", "-co", "--exclude-standard", "--", "*.json"], { cwd: REPO, encoding: "utf8" })
      .split("\n").filter(Boolean);
    const holders = files.filter((f) => {
      try { return holdsIngressBody(JSON.parse(readFileSync(resolve(REPO, f), "utf8"))); } catch { return false; }
    });
    expect(holders).toEqual([INGRESS_WORKFLOW_REL]);
  });

  it("the old copy is a pointer stub to the live file", () => {
    const stub = JSON.parse(readFileSync(resolve(REPO, "stack/integrations/n8n/terrance_ingress_workflow.json"), "utf8"));
    expect(stub.nodes).toBeUndefined();
    expect(stub._readme.join(" ")).toContain(INGRESS_WORKFLOW_REL);
  });
});
