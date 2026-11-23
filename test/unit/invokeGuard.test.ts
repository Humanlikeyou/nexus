// @vitest-environment node
// Typed bridge guard (decision 0023, step 6): the core `invoke` from @tauri-apps/api/core may be used only in
//  - src-console/bindings.gen.ts (generated, imports it as __TAURI_INVOKE), and
//  - src-console/bridge.ts, in exactly two documented raw cases: the ha_call_service "no data key" call inside
//    invokeBound, and pluginDialogOpen (tauri-plugin-dialog's `open` is a plugin command with no app binding).
// Everything else calls bridge functions, invokeBound, or `commands.*` from the bindings. Tests are not scanned
// (they mock the core module).
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const ROOT = join(__dirname, "../..");
const GENERATED = "src-console/bindings.gen.ts";
const BRIDGE = "src-console/bridge.ts";

/** Patterns that reach the core invoke by any route. */
const IMPORT_FROM_CORE = /\bfrom\s*["']@tauri-apps\/api\/core["']|import\(\s*["']@tauri-apps\/api\/core["']\s*\)|require\(\s*["']@tauri-apps\/api\/core["']\s*\)/;
const CALL = /(?<![\w.$])(?:invoke|__TAURI_INVOKE)\s*(?:<|\()|\b\w+\.invoke\s*(?:<|\()|__TAURI_INTERNALS__\s*(?:\?\.|\.|\[)/;

/** The only two call shapes allowed in bridge.ts. */
const BRIDGE_ALLOWED: RegExp[] = [
  /invoke<T>\(command, \{ domain, service, entityId \}\)/, // ha_call_service without data
  /invoke<T>\("plugin:dialog\|open", \{ options \}\)/, // pluginDialogOpen
];

/** Violations in one file's source: raw core imports and raw invoke calls, minus the bridge allowlist. */
export function violations(file: string, src: string): string[] {
  if (file === GENERATED) return [];
  const out: string[] = [];
  const lines = src.split("\n");
  lines.forEach((line, i) => {
    const code = line.replace(/\/\/.*$/, "");
    if (/^\s*(\*|\/\*)/.test(line)) return; // doc comment lines
    const hitsImport = IMPORT_FROM_CORE.test(code) && /\binvoke\b|\* as\b|import\(|require\(/.test(code);
    const hitsCall = CALL.test(code);
    if (!hitsImport && !hitsCall) return;
    if (file === BRIDGE && hitsCall && BRIDGE_ALLOWED.some((re) => re.test(code))) return;
    if (file === BRIDGE && hitsImport && /^import \{ Channel, invoke \} from "@tauri-apps\/api\/core";$/.test(code.trim())) return;
    out.push(`${file}:${i + 1}: ${line.trim()}`);
  });
  return out;
}

function walk(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "node_modules" ? [] : walk(p);
    return [p];
  });
}

const isSource = (rel: string) => /\.(ts|tsx|mjs|js|jsx)$/.test(rel) && !/\.test\.(ts|tsx|mjs|js|jsx)$/.test(rel);

describe("no raw invoke outside the generated bindings and the bridge's two documented cases", () => {
  it("scans every non-test source file under src-console and src", () => {
    const files = [...walk(join(ROOT, "src-console")), ...walk(join(ROOT, "src"))].map((f) => relative(ROOT, f)).filter(isSource);
    expect(files.length).toBeGreaterThan(100);
    const bad = files.flatMap((f) => violations(f, readFileSync(join(ROOT, f), "utf8")));
    expect(bad).toEqual([]);
  });

  it("bridge.ts still has exactly the two allowed raw calls", () => {
    const src = readFileSync(join(ROOT, BRIDGE), "utf8");
    const calls = src.split("\n").map((l) => l.replace(/\/\/.*$/, "")).filter((l) => !/^\s*(\*|\/\*)/.test(l) && CALL.test(l));
    expect(calls.length).toBe(2);
    for (const re of BRIDGE_ALLOWED) expect(calls.some((c) => re.test(c))).toBe(true);
  });

  // Fail-before proof: fixtures that the old code shape would have been (and that a regression would reintroduce).
  it("flags a raw invoke in an ordinary module", () => {
    const fixture = `import { invoke } from "@tauri-apps/api/core";\nexport const f = () => invoke<string>("get_config");\n`;
    expect(violations("src-console/legacy-core/lib/api.ts", fixture)).toHaveLength(2);
  });

  it("flags namespace imports, dynamic imports, a call on a namespace, and the internals object", () => {
    expect(violations("src-console/x.ts", `import * as core from "@tauri-apps/api/core";\ncore.invoke("a");`)).toHaveLength(2);
    expect(violations("src-console/x.ts", `const m = await import("@tauri-apps/api/core");`)).toHaveLength(1);
    expect(violations("src-console/x.ts", `window.__TAURI_INTERNALS__.invoke("a");`)).toHaveLength(1);
    expect(violations("src-console/x.ts", `import { invoke as call } from "@tauri-apps/api/core";`)).toHaveLength(1);
  });

  it("flags a third raw call in bridge.ts but not the two documented ones", () => {
    const ok = `import { Channel, invoke } from "@tauri-apps/api/core";\n  return invoke<T>(command, { domain, service, entityId });\n  return invoke<T>("plugin:dialog|open", { options });\n`;
    expect(violations(BRIDGE, ok)).toEqual([]);
    expect(violations(BRIDGE, `${ok}  return invoke<T>("get_config");\n`)).toHaveLength(1);
  });

  it("does not flag imports of other tauri modules, Channel alone, or the generated file", () => {
    expect(violations("src-console/x.ts", `import { emit } from "@tauri-apps/api/event";\nimport { Channel } from "@tauri-apps/api/core";`)).toEqual([]);
    expect(violations(GENERATED, `import { invoke as __TAURI_INVOKE } from "@tauri-apps/api/core";\n__TAURI_INVOKE("a");`)).toEqual([]);
  });
});
