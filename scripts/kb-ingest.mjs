#!/usr/bin/env node
/**
 * Safe knowledge-base ingest (plan: stack/kb/INGEST-PLAN.md).
 *
 * Default --dry-run: lists the files that would be ingested (bytes), every skip with its
 * reason, and totals. Writes nothing, sends nothing.
 * --apply: needs Verba healthy, local-only (no remote provider or key in the verba
 * container), check:secrets clean, and at most 200 files / 5 MB; then posts each file as
 * kb.ingest to the local Ingress with NEXUS_SECRET from stack/.env (never printed).
 *
 * Usage: scripts/kb-ingest.sh [--dry-run | --apply] [--repo <dir>] [--verba-env-file <file>]
 *   --verba-env-file  read the verba env from a file (tests) instead of `docker exec`.
 */
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { scanText, parseAllowlist } from "./check-secrets.mjs";

export const MAX_FILES = 200;
export const MAX_BYTES = 5 * 1024 * 1024;
const VERBA_CONTAINER = process.env.VERBA_CONTAINER ?? "stack-verba-1";
const VERBA_URL = process.env.VERBA_URL ?? "http://127.0.0.1:8001";
const INGRESS_URL = process.env.INGRESS_URL ?? "http://127.0.0.1:5678/webhook/terrance/ingress";

/** Allowlist (repo-relative). `**` = any depth, `*` = within one segment. */
export const ALLOW = [
  "README.md", "CLAUDE.md", "STAGES.md", "BUILD.md",
  "docs/**/*.md",
  "stack/SECURITY-NOTES.md",
  "stack/crew-service/README*",
  "stack/n8n/**/*.md",
];

/** Denylist: [reason, test(relPath)]. Checked before anything else; beats the allowlist. */
const seg = (re) => (p) => p.split("/").some((s) => re.test(s));
export const DENY = [
  ["env file", seg(/^\.env(\..*)?$/i)],
  ["key/cert file", (p) => /\.(key|pem|p12|pfx)$/i.test(p)],
  ["credentials", (p) => /credential/i.test(p)],
  ["secrets dir", seg(/^secrets?$/i)],
  ["stack data", (p) => p.startsWith("stack/data/")],
  ["database", (p) => /\.(sqlite3?|db)(-wal|-shm)?$/i.test(p)],
  ["health data (OpenWhoop/NOOP/whoop/strain/recovery/sleep/HRV)", (p) => /openwhoop|noop|whoop|strain|recovery|sleep|\bhrv\b|hrv[_.-]/i.test(p)],
  ["neuro data (Muse/EEG/neuro)", (p) => /muse|\beeg\b|eeg[_.-]|neuro/i.test(p)],
  ["data export (.csv/.json)", (p) => /\.(csv|json|jsonl|ndjson)$/i.test(p)],
  ["log", (p) => /\.log$/i.test(p) || p.split("/").some((s) => /^logs?$/i.test(s))],
  ["backup", (p) => /backup|\.bak$|\.orig$/i.test(p)],
  ["archive", (p) => p.split("/").some((s) => /^archive$/i.test(s)) || p.includes("/Archive/")],
];

/** Secret patterns beyond check:secrets (which covers vendor keys and high-entropy assignments). */
export const EXTRA_SECRET = [
  ["bearer-token", /\bBearer\s+[A-Za-z0-9._~+/-]{20,}=*/],
  ["jwt", /\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/],
  ["sk-key", /\bsk-[A-Za-z0-9_-]{16,}/],
  ["api-key", /\b(?:api[_-]?key|apikey|access[_-]?token|auth[_-]?token|secret)\b\s*[:=]\s*["']?[A-Za-z0-9/+_.~-]{16,}/i],
  ["password", /\bpass(?:word|wd)?\b\s*[:=]\s*["']?[^\s"'<>{}$]{8,}/i],
  ["private-key", /-----BEGIN (?:[A-Z]+ )*PRIVATE KEY-----/],
];

/** Remote-provider key env vars that must be unset in the verba container. */
export const REMOTE_KEYS = [
  "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "COHERE_API_KEY", "GROQ_API_KEY", "NOVITA_API_KEY",
  "UPSTAGE_API_KEY", "VOYAGE_API_KEY", "GOOGLE_API_KEY", "GEMINI_API_KEY", "MISTRAL_API_KEY",
  "HUGGINGFACE_API_KEY", "UNSTRUCTURED_API_KEY", "ASSEMBLYAI_API_KEY", "FIRECRAWL_API_KEY",
  "GITHUB_TOKEN", "GITLAB_TOKEN",
];
/** URL env vars that must point at this machine or the compose network. */
const URL_VARS = ["OLLAMA_URL", "WEAVIATE_URL_VERBA", "OPENAI_BASE_URL"];
const LOCAL_HOSTS = /^(localhost|127\.0\.0\.1|::1|\[::1\]|host\.docker\.internal|weaviate|verba|ollama|n8n)$/i;

function globRe(glob) {
  let re = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") { re += "(?:.*/)?"; i += glob[i + 2] === "/" ? 2 : 1; }
    else if (c === "*") re += "[^/]*";
    else re += c.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${re}$`);
}
const ALLOW_RE = ALLOW.map(globRe);
export const allowlisted = (rel) => ALLOW_RE.some((re) => re.test(rel));
export const denyReason = (rel) => DENY.find(([, t]) => t(rel))?.[0] ?? null;

/** Every file under the allowlisted roots (no symlinks followed), repo-relative. */
function candidates(repo) {
  const out = [];
  const walk = (dir) => {
    let entries;
    try { entries = fs.readdirSync(path.join(repo, dir), { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      const rel = dir ? `${dir}/${e.name}` : e.name;
      if (e.isSymbolicLink()) { out.push({ rel, symlink: true }); continue; }
      // stack/data is denied wholesale: listed once, never walked.
      if (e.isDirectory() && rel === "stack/data") { out.push({ rel: `${rel}/`, dir: true }); continue; }
      if (e.isDirectory()) { if (e.name !== "node_modules" && e.name !== ".git") walk(rel); continue; }
      if (e.isFile()) out.push({ rel });
    }
  };
  for (const top of ["", "docs", "stack"]) {
    if (top === "") {
      for (const e of fs.readdirSync(repo, { withFileTypes: true })) if (!e.isDirectory()) out.push({ rel: e.name, symlink: e.isSymbolicLink() });
    } else walk(top);
  }
  return out;
}

export function secretHits(rel, text, allow = []) {
  const hits = scanText(rel, text, allow).map((f) => f.pattern);
  for (const [name, re] of EXTRA_SECRET) if (re.test(text)) hits.push(name);
  return [...new Set(hits)];
}

/** Pure planning: what would be ingested and what is skipped (with reasons). Reads only. */
export function plan(repo) {
  const allowFile = path.join(repo, ".secrets-allow");
  const allow = fs.existsSync(allowFile) ? parseAllowlist(fs.readFileSync(allowFile, "utf8")) : [];
  const files = [], skipped = [], denied = {};
  for (const c of candidates(repo)) {
    const deny = denyReason(c.rel);
    // Not allowlisted: never a candidate. Denied ones are only counted (by reason).
    if (!allowlisted(c.rel)) { if (deny) denied[deny] = (denied[deny] ?? 0) + 1; continue; }
    if (deny) { skipped.push({ rel: c.rel, reason: `denylist: ${deny}` }); continue; }
    if (c.symlink) { skipped.push({ rel: c.rel, reason: "symlink" }); continue; }
    const buf = fs.readFileSync(path.join(repo, c.rel));
    if (buf.includes(0)) { skipped.push({ rel: c.rel, reason: "binary" }); continue; }
    const hits = secretHits(c.rel, buf.toString("utf8"), allow);
    if (hits.length) { skipped.push({ rel: c.rel, reason: `secret scan: ${hits.join(", ")}` }); continue; }
    files.push({ rel: c.rel, bytes: buf.length });
  }
  files.sort((a, b) => a.rel.localeCompare(b.rel));
  skipped.sort((a, b) => a.rel.localeCompare(b.rel));
  return { files, skipped, denied, totalBytes: files.reduce((n, f) => n + f.bytes, 0) };
}

export function parseEnv(text) {
  const env = {};
  for (const line of text.split("\n")) {
    const m = line.match(/^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)=(.*)$/);
    if (m) env[m[1]] = m[2].trim().replace(/^(["'])(.*)\1$/, "$2");
  }
  return env;
}

/** Local-only check of the verba env. Returns problems by NAME only (values never shown). */
export function remoteProblems(env) {
  const problems = [];
  for (const k of REMOTE_KEYS) if (env[k]) problems.push(`${k} is set`);
  for (const k of URL_VARS) {
    if (!env[k]) continue;
    let host = "";
    try { host = new URL(env[k]).hostname; } catch { problems.push(`${k} is not a URL`); continue; }
    if (!LOCAL_HOSTS.test(host)) problems.push(`${k} points at a remote host`);
  }
  return problems;
}

function verbaEnv(envFile) {
  if (envFile) return parseEnv(fs.readFileSync(envFile, "utf8"));
  const out = execFileSync("docker", ["exec", VERBA_CONTAINER, "env"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  return parseEnv(out);
}

function checkSecretsGate(repo) {
  if (!fs.existsSync(path.join(repo, "scripts/check-secrets.mjs"))) return null;
  try {
    execFileSync(process.execPath, [path.join(repo, "scripts/check-secrets.mjs")], { cwd: repo, stdio: "ignore" });
    return true;
  } catch { return false; }
}

const kb = (n) => `${(n / 1024).toFixed(1)} KB`;

export async function main(argv, log = console.log) {
  const args = { apply: false, repo: path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".."), envFile: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--dry-run") args.apply = false;
    else if (a === "--apply") args.apply = true;
    else if (a === "--repo") args.repo = path.resolve(argv[++i]);
    else if (a === "--verba-env-file") args.envFile = argv[++i];
    else { log(`kb-ingest: unknown argument: ${a}`); return 2; }
  }
  log(`kb-ingest ${args.apply ? "--apply" : "--dry-run"} (repo ${args.repo})`);

  // Local only: abort on any remote provider or key in the verba container.
  let env = null;
  try { env = verbaEnv(args.envFile); } catch { env = null; }
  if (env) {
    const problems = remoteProblems(env);
    if (problems.length) {
      log(`ABORT: Verba is not local-only: ${problems.join("; ")}`);
      return 3;
    }
    log("verba env: local only (no remote provider keys or URLs)");
  } else if (args.apply) {
    log("ABORT: cannot read the verba container env (is it running?)");
    return 3;
  } else {
    log("verba env: not checked (container not reachable); --apply would abort");
  }

  const gate = checkSecretsGate(args.repo);
  log(`check:secrets: ${gate === null ? "not run (no scripts/check-secrets.mjs in this repo)" : gate ? "clean" : "FINDINGS (run npm run check:secrets)"}`);

  const p = plan(args.repo);
  log("");
  log(`Would ingest ${p.files.length} files:`);
  for (const f of p.files) log(`  ${f.rel}  ${f.bytes} B`);
  log("");
  log(`Skipped ${p.skipped.length} allowlisted files:`);
  for (const s of p.skipped) log(`  ${s.rel}  (${s.reason})`);
  log("Denylisted outside the allowlist (never candidates):");
  for (const [reason, n] of Object.entries(p.denied).sort()) log(`  ${n}  ${reason}`);
  log("");
  log(`Totals: ${p.files.length} files, ${p.totalBytes} B (${kb(p.totalBytes)}); caps ${MAX_FILES} files / ${kb(MAX_BYTES)}`);

  if (!args.apply) { log("Dry run: nothing written or sent."); return 0; }

  if (!gate) { log("ABORT: check:secrets has findings"); return 4; }
  if (p.files.length > MAX_FILES || p.totalBytes > MAX_BYTES) { log("ABORT: over the 200 files / 5 MB cap"); return 4; }
  try {
    const r = await fetch(`${VERBA_URL}/api/health`, { signal: AbortSignal.timeout(5000) });
    if (!r.ok) throw new Error(String(r.status));
  } catch { log("ABORT: Verba is not healthy"); return 5; }
  const secret = parseEnv(fs.readFileSync(path.join(args.repo, "stack/.env"), "utf8")).NEXUS_SECRET;
  if (!secret) { log("ABORT: NEXUS_SECRET is not set in stack/.env"); return 5; }

  let ok = 0;
  for (const f of p.files) {
    const content = fs.readFileSync(path.join(args.repo, f.rel), "utf8");
    const body = { trace_id: `kb-ingest-${Date.now()}`, event: { type: "kb.ingest", payload: { filename: f.rel, content, labels: ["nexus-docs"] } } };
    try {
      const r = await fetch(INGRESS_URL, { method: "POST", headers: { "Content-Type": "application/json", "X-Nexus-Secret": secret }, body: JSON.stringify(body), signal: AbortSignal.timeout(60_000) });
      const j = await r.json().catch(() => ({}));
      if (r.ok && j.ok === true) { ok++; log(`  ingested ${f.rel}`); }
      else log(`  FAILED ${f.rel} (HTTP ${r.status}${j?.error?.message ? `: ${String(j.error.message).slice(0, 80)}` : ""})`);
    } catch { log(`  FAILED ${f.rel} (ingress unreachable)`); }
  }
  log(`Ingested ${ok}/${p.files.length}.`);
  return ok === p.files.length ? 0 : 6;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).then((code) => process.exit(code));
}
