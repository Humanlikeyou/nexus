// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");

describe("crew approvals and Terrance's queue stay separate (source check)", () => {
  it("the crew module never touches Terrance's commands, and Terrance's reads never touch crew approvals", () => {
    const crew = read("../../src-console/crews/approvals.ts");
    expect(crew).not.toMatch(/list_terrence_actions|approve_terrence_action|cancelTerrenceAction|TerrenceAction/);
    const bridge = read("../../src-console/bridge.ts");
    const snapshot = bridge.slice(bridge.indexOf("export async function loadSystemSnapshot"), bridge.indexOf("export function crewSnapshot"));
    expect(snapshot.length).toBeGreaterThan(500);
    expect(snapshot).not.toMatch(/crew_list_approvals|crew_decide_approval/);
    const ledger = bridge.slice(bridge.indexOf("approveTerrenceAction"), bridge.indexOf("approveTerrenceAction") + 200);
    expect(ledger).not.toMatch(/crew_/);
  });
});
