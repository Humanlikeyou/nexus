import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

// Audit 2026-11-19: a class that sets `display` must not land on a closed <dialog>, or the dialog shows inline.
// .d18-confirm is shared by the inline delete row (GoalEditors) and the trading confirm <dialog> (Trading.tsx).
const css = fs.readFileSync(path.resolve(__dirname, "../../src-console/design1118/design1118.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");

describe("d18-confirm display rule", () => {
  it("never sets display on a <dialog>", () => {
    const rules = [...css.matchAll(/([^{}]*\.d18-confirm[^{}]*)\{([^}]*)\}/g)].filter((m) => /display\s*:/.test(m[2]));
    expect(rules.length).toBeGreaterThan(0);
    for (const m of rules) expect(m[1]).toMatch(/:not\(dialog\)/);
  });
});
