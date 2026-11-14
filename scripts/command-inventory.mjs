#!/usr/bin/env node
/**
 * Tauri command inventory (docs/terrance-merge-inventory.md).
 *
 *  - Registered commands: parsed from generate_handler![...] in src-tauri/src/lib.rs.
 *  - Live code: every module in src-console/main.tsx's import graph (esbuild
 *    metafile, with the vite @ / @console aliases), including the src/ modules
 *    it pulls in (scheduler, continuity, rituals, …).
 *  - Callers: `invoke("name"` / `read("name"` / `__TAURI_INVOKE("name"` (generated src-console/bindings.gen.ts) calls (generic args and line
 *    breaks allowed) in src-console/, src/ and src-next/ (tests excluded).
 *
 * Usage:
 *   node scripts/command-inventory.mjs            # JSON to stdout
 *   node scripts/command-inventory.mjs --markdown # tables used in the doc
 *
 * Also importable: `import { inventory } from "./command-inventory.mjs"`.
 */
import { build } from "esbuild";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const LIB_RS = path.join(ROOT, "src-tauri/src/lib.rs");
const ENTRY = path.join(ROOT, "src-console/main.tsx");
const FRONTEND_DIRS = ["src-console", "src", "src-next"];
const CODE_EXT = /\.(ts|tsx|js|jsx|mjs)$/;
const TEST_FILE = /\.test\.(ts|tsx|js|jsx|mjs)$/;

/** `module::fn` entries of generate_handler![...] (comments stripped). */
export function registeredCommands(libRs = fs.readFileSync(LIB_RS, "utf8")) {
  const start = libRs.indexOf("generate_handler![");
  if (start < 0) throw new Error("generate_handler! not found in lib.rs");
  const end = libRs.indexOf("]", start);
  return libRs
    .slice(start + "generate_handler![".length, end)
    .replace(/\/\/[^\n]*/g, "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
    .map((full) => ({ full, name: full.split("::").pop() }));
}

/** Command names a source file calls via invoke(...) or the bridge's read(...). */
export function commandCalls(source) {
  const names = new Set();
  // Generic args may nest one level and contain parens: invoke<import("./types").X[]>("name").
  const re = /\b(?:invoke|read|__TAURI_INVOKE)\s*(?:<(?:[^<>]|<[^<>]*>)*>)?\s*\(\s*["'`]([a-z][a-z0-9_]*)["'`]/g;
  for (const m of source.matchAll(re)) names.add(m[1]);
  return [...names];
}

function walk(dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(p));
    else out.push(p);
  }
  return out;
}

/** Repo-relative paths of every module reachable from src-console/main.tsx. */
export async function reachableFiles() {
  const result = await build({
    entryPoints: [ENTRY],
    bundle: true,
    write: false,
    metafile: true,
    logLevel: "silent",
    format: "esm",
    platform: "browser",
    jsx: "automatic",
    alias: { "@": path.join(ROOT, "src"), "@console": path.join(ROOT, "src-console") },
    loader: { ".css": "empty", ".svg": "empty", ".woff": "empty", ".woff2": "empty", ".png": "empty", ".ttf": "empty", ".otf": "empty" },
    define: { __BUILD_HASH__: '""', __BUILD_TIME__: '""', __APP_VERSION__: '""', __TAURI_VERSION__: '""' },
    absWorkingDir: ROOT,
  });
  return new Set(Object.keys(result.metafile.inputs).filter((p) => !p.startsWith("node_modules/")));
}

export async function inventory() {
  const registered = registeredCommands();
  const live = await reachableFiles();
  const files = FRONTEND_DIRS.flatMap((d) => walk(path.join(ROOT, d)))
    .map((abs) => path.relative(ROOT, abs))
    .filter((rel) => CODE_EXT.test(rel) && !TEST_FILE.test(rel))
    .sort();

  /** command → { live: [files], legacy: [files] } */
  const callers = new Map();
  const referenced = new Map(); // every called name → files (incl. unregistered)
  for (const rel of files) {
    const names = commandCalls(fs.readFileSync(path.join(ROOT, rel), "utf8"));
    for (const name of names) {
      if (!referenced.has(name)) referenced.set(name, []);
      referenced.get(name).push(rel);
      if (!callers.has(name)) callers.set(name, { live: [], legacy: [] });
      callers.get(name)[live.has(rel) ? "live" : "legacy"].push(rel);
    }
  }

  const commands = registered.map(({ full, name }) => {
    const c = callers.get(name) ?? { live: [], legacy: [] };
    const status = c.live.length ? "live" : c.legacy.length ? "legacy-only" : "no-callers";
    return { name, handler: full, status, liveCallers: c.live, legacyCallers: c.legacy };
  });

  // Every file under src/ and src-next/ (tests included, marked as such).
  const legacyFiles = ["src", "src-next"]
    .flatMap((d) => walk(path.join(ROOT, d)))
    .map((abs) => path.relative(ROOT, abs))
    .sort()
    .map((rel) => ({ file: rel, reachable: live.has(rel), test: TEST_FILE.test(rel) }));

  const registeredNames = new Set(registered.map((r) => r.name));
  const unregistered = [...referenced.entries()]
    .filter(([name]) => !registeredNames.has(name))
    .map(([name, inFiles]) => ({ name, files: inFiles, liveFiles: inFiles.filter((f) => live.has(f)) }));

  return { commands, legacyFiles, unregistered, liveFileCount: live.size };
}

function markdown(inv) {
  const lines = [];
  const counts = inv.commands.reduce((acc, c) => ({ ...acc, [c.status]: (acc[c.status] ?? 0) + 1 }), {});
  lines.push(`Registered: ${inv.commands.length} · live: ${counts.live ?? 0} · legacy-only: ${counts["legacy-only"] ?? 0} · no callers: ${counts["no-callers"] ?? 0}`, "");
  lines.push("| Command | Status | Live callers | Legacy callers |", "|---|---|---|---|");
  for (const c of inv.commands) {
    lines.push(`| \`${c.name}\` | ${c.status} | ${c.liveCallers.map((f) => `\`${f}\``).join(", ") || "—"} | ${c.legacyCallers.map((f) => `\`${f}\``).join(", ") || "—"} |`);
  }
  lines.push("", "| File | Reachable from main.tsx |", "|---|---|");
  for (const f of inv.legacyFiles) lines.push(`| \`${f.file}\` | ${f.reachable ? "yes" : f.test ? "no (test file)" : "no"} |`);
  if (inv.unregistered.length) {
    lines.push("", "Referenced but not registered:", "");
    for (const u of inv.unregistered) lines.push(`- \`${u.name}\`: ${u.files.map((f) => `\`${f}\``).join(", ")}${u.liveFiles.length ? " (LIVE)" : ""}`);
  }
  return lines.join("\n");
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const inv = await inventory();
  process.stdout.write(process.argv.includes("--markdown") ? `${markdown(inv)}\n` : `${JSON.stringify(inv, null, 2)}\n`);
}
