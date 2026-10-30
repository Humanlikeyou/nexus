#!/usr/bin/env node
/**
 * Secret-hygiene gate: scans tracked files (`git ls-files`) and fails on
 *  - a tracked .env / .env.* file other than .env.example and .env.test, or
 *  - a line matching a vendor key pattern or a high-entropy value assigned to
 *    *_KEY / *_SECRET / *_TOKEN / *PASSWORD.
 * Findings print as `file:line  pattern` only: never the matched text.
 *
 * Allowlist: .secrets-allow at the repo root, one glob per line (`*` = any run of
 * [A-Za-z0-9_-]; `#` starts a comment). A match is allowed when a glob is found
 * inside the matched text, e.g. `FAKE_*_NOT_REAL` for known-fake fixtures.
 *
 * Usage: npm run check:secrets   (exit 1 on findings)
 * Also importable: `import { scanText, forbiddenEnvFile } from "./check-secrets.mjs"`.
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAX_BYTES = 2 * 1024 * 1024;
const ASSIGNED_MIN_BITS = 3.5; // Shannon bits per char; random base64/hex keys sit at ~3.7-6

/** Vendor key patterns: [name, regex]. Global regexes so every match on a line is checked. */
export const PATTERNS = [
  ["anthropic-key", /\bsk-ant-[A-Za-z0-9_-]{20,}/g],
  ["openai-key", /\bsk-(?!ant-)(?:proj-)?[A-Za-z0-9_-]{20,}/g],
  ["github-token", /\bghp_[A-Za-z0-9]{36}\b/g],
  ["github-pat", /\bgithub_pat_[A-Za-z0-9_]{22,}/g],
  ["slack-token", /\bxox[bap]-[A-Za-z0-9-]{10,}/g],
  ["aws-access-key", /\bAKIA[0-9A-Z]{16}\b/g],
  ["telegram-bot-token", /(?<![\w:])\d{8,10}:[A-Za-z0-9_-]{35}(?![A-Za-z0-9_-])/g],
  ["pem-private-key", /-----BEGIN (?:[A-Z]+ )*PRIVATE KEY-----/g],
];

/** `NAME = value` / `"name": "value"` where NAME ends in _KEY, _SECRET, _TOKEN or PASSWORD. */
const ASSIGNED = /["']?\b([A-Za-z0-9_]*(?:_KEY|_SECRET|_TOKEN|PASSWORD))["']?\s*[:=]\s*["']?([A-Za-z0-9+/=_.-]{32,})/gi;
/** Dotted letters-only names (storage keys like `app.section.someKey`) are identifiers, not secrets. */
const DOTTED_IDENTIFIER = /^[A-Za-z]+(?:\.[A-Za-z]+)+$/;

/** Shannon entropy in bits per character. */
export function entropy(s) {
  const counts = new Map();
  for (const ch of s) counts.set(ch, (counts.get(ch) ?? 0) + 1);
  let bits = 0;
  for (const n of counts.values()) bits -= (n / s.length) * Math.log2(n / s.length);
  return bits;
}

/** Parse .secrets-allow text into regexes. */
export function parseAllowlist(text) {
  return text
    .split("\n")
    .map((l) => l.replace(/#.*/, "").trim())
    .filter(Boolean)
    .map((glob) => new RegExp(glob.split("*").map((p) => p.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join("[A-Za-z0-9_-]*")));
}

/** True for a tracked file name that must never be committed. */
export function forbiddenEnvFile(file) {
  const base = path.posix.basename(file);
  return (base === ".env" || base.startsWith(".env.")) && base !== ".env.example" && base !== ".env.test";
}

/** Findings for one file's text: [{ file, line, pattern }]. Never includes the matched text. */
export function scanText(file, text, allow = []) {
  const findings = [];
  const allowed = (s) => allow.some((re) => re.test(s));
  text.split("\n").forEach((line, i) => {
    for (const [name, re] of PATTERNS) {
      for (const m of line.matchAll(re)) {
        if (!allowed(m[0])) findings.push({ file, line: i + 1, pattern: name });
      }
    }
    for (const m of line.matchAll(ASSIGNED)) {
      const value = m[2];
      if (!DOTTED_IDENTIFIER.test(value) && entropy(value) >= ASSIGNED_MIN_BITS && !allowed(value)) {
        findings.push({ file, line: i + 1, pattern: "assigned-high-entropy" });
      }
    }
  });
  // One report per file:line:pattern.
  const seen = new Set();
  return findings.filter((f) => {
    const k = `${f.line}:${f.pattern}`;
    return seen.has(k) ? false : (seen.add(k), true);
  });
}

/** Scan the tracked files under `root`. */
export function scanRepo(root = ROOT) {
  const files = execFileSync("git", ["ls-files", "-z"], { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 })
    .split("\0")
    .filter(Boolean);
  const allowFile = path.join(root, ".secrets-allow");
  const allow = fs.existsSync(allowFile) ? parseAllowlist(fs.readFileSync(allowFile, "utf8")) : [];
  const findings = [];
  for (const file of files) {
    if (forbiddenEnvFile(file)) findings.push({ file, line: 0, pattern: "tracked-env-file" });
    if (file === ".secrets-allow") continue;
    const abs = path.join(root, file);
    let buf;
    try {
      const st = fs.lstatSync(abs);
      if (!st.isFile() || st.size > MAX_BYTES) continue;
      buf = fs.readFileSync(abs);
    } catch {
      continue; // deleted in the working tree
    }
    if (buf.includes(0)) continue; // binary
    findings.push(...scanText(file, buf.toString("utf8"), allow));
  }
  return { files: files.length, findings };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const { files, findings } = scanRepo();
  for (const f of findings) console.log(`${f.file}${f.line ? `:${f.line}` : ""}  ${f.pattern}`);
  console.log(findings.length ? `check:secrets: ${findings.length} finding(s) in ${files} tracked files` : `check:secrets: ok (${files} tracked files)`);
  process.exit(findings.length ? 1 : 0);
}
