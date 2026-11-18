// Phase 16: shipped source must not hard-code a path of the developer's Mac. Everything is relative to
// app-data (see src-tauri/src/app_paths.rs). Tests, docs and fixtures may name a home folder; shipped
// code may not.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const root = resolve(__dirname, "../..");
const ROOTS = [
  "src-tauri/src",
  "src-tauri/capabilities",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-console",
  "src",
  "amara-router/src",
  "amara-router/Cargo.toml",
  "scripts",
  "stack",
];
const SKIP_DIR = new Set(["node_modules", "target", ".git", "__pycache__", "dist", ".venv"]);
const SKIP_FILE = /\.(md|png|jpg|jpeg|gif|ico|icns|woff2?|ttf|otf|svg|lock|gz|db|sqlite)$|\.test\.[a-z]+$|^test_.*\.py$|\.spec\.[a-z]+$/;
// The developer's real home folder or user name. Fake users in unit tests ("/Users/x") are not matched.
const FORBIDDEN = [/\/Users\/thevisualdude/i, /\/Users\/taisun/i, /\/home\/thevisualdude/i];
// launchd label of the user's existing voice service, an identifier and not a path.
const ALLOWED_LINES = [/com\.thevisualdude\.(amara-voice|open-webui)/];

function* walk(p: string): Generator<string> {
  const st = statSync(p, { throwIfNoEntry: false });
  if (!st) return;
  if (st.isFile()) {
    yield p;
    return;
  }
  for (const name of readdirSync(p)) {
    if (SKIP_DIR.has(name)) continue;
    yield* walk(join(p, name));
  }
}

describe("no absolute path of this Mac in shipped source", () => {
  it("finds none under src-tauri/src, src-console (non-test), amara-router/src, scripts, stack, tauri.conf.json", () => {
    const hits: string[] = [];
    let scanned = 0;
    for (const r of ROOTS) {
      for (const f of walk(join(root, r))) {
        if (SKIP_FILE.test(f.split("/").pop() ?? "")) continue;
        scanned += 1;
        readFileSync(f, "utf8")
          .split("\n")
          .forEach((line, i) => {
            if (ALLOWED_LINES.some((a) => a.test(line))) return;
            if (FORBIDDEN.some((re) => re.test(line))) hits.push(`${relative(root, f)}:${i + 1}`);
          });
      }
    }
    expect(scanned).toBeGreaterThan(200); // the walk really covered the tree
    expect(hits).toEqual([]);
  });

  it("the scan itself catches a real path (self-check of the patterns)", () => {
    expect(FORBIDDEN.some((re) => re.test('let p = "/Users/thevisualdude/projects/x";'))).toBe(true);
    expect(FORBIDDEN.some((re) => re.test('Path::new("/Users/x/Applications")'))).toBe(false);
  });
});
