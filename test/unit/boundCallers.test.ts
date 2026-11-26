// @vitest-environment node
// invokeBound rejects any name missing from BOUND_TABLE (decision 0023). A command registered in lib.rs and called by name
// from the console but never bound therefore fails in the app while every injected-`call` unit test still passes: that is
// how habit marks, roadmap answers, Letta memory writes and Hemi-Sync sessions broke after 371e0d2 (fixed in decision 0056).
// This scans every non-test console source for a quoted registered command name and requires a bound row for it.
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { BOUND_TABLE } from "../../src-console/boundTable";

const ROOT = join(__dirname, "../..");
/** Registered commands the console names in a string without invoking them through invokeBound. */
const NOT_CALLS = new Set<string>([
  "amara_turn", // Channel call, bound by hand in bridge.ts
]);

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
}

export function registered(lib: string): Set<string> {
  const block = lib.slice(lib.indexOf("generate_handler!["));
  return new Set([...block.slice(0, block.indexOf("])")).matchAll(/(?:::|\s)(\w+),/g)].map((m) => m[1]).filter((n) => /^[a-z][a-z0-9_]*$/.test(n)));
}

export function unboundCalls(files: Array<[string, string]>, reg: Set<string>, bound: Set<string>): string[] {
  const out = new Set<string>();
  for (const [file, raw] of files) {
    // Comments only mention commands; strip block comments and line comments (not "://" inside URLs).
    const src = raw.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1");
    for (const m of src.matchAll(/["'`]([a-z][a-z0-9]*(?:_[a-z0-9]+)+)["'`]/g)) {
      const name = m[1];
      if (reg.has(name) && !bound.has(name) && !NOT_CALLS.has(name)) out.add(`${name} (${file})`);
    }
  }
  return [...out].sort();
}

describe("every registered command the console calls by name has a bound row", () => {
  const reg = registered(readFileSync(join(ROOT, "src-tauri/src/lib.rs"), "utf8"));
  it("reads the registry", () => { expect(reg.size).toBeGreaterThan(300); expect(reg.has("habit_mark")).toBe(true); });
  it("the check catches a registered, unbound name (fail-before proof)", () => {
    expect(unboundCalls([["x.ts", `call("habit_mark", {})`]], new Set(["habit_mark"]), new Set())).toEqual(["habit_mark (x.ts)"]);
    expect(unboundCalls([["x.ts", `call("habit_mark", {})`]], new Set(["habit_mark"]), new Set(["habit_mark"]))).toEqual([]);
  });
  it("no console source names an unbound registered command", () => {
    const files = walk(join(ROOT, "src-console"))
      .filter((f) => /\.(ts|tsx)$/.test(f) && !/\.test\.(ts|tsx)$/.test(f) && !f.endsWith("bindings.gen.ts"))
      .map((f) => [relative(ROOT, f), readFileSync(f, "utf8")] as [string, string]);
    expect(unboundCalls(files, reg, new Set(Object.keys(BOUND_TABLE)))).toEqual([]);
  });
});
