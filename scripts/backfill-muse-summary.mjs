#!/usr/bin/env node
// Backfill avg_focus / avg_relaxation / calm_pct for saved Muse sessions whose summary is
// all-null (plain recordings saved before the recording summarizer existed).
// Dry run by default; pass --apply to write. Replay ndjson files are only read.
// Each session json is backed up (0600, in session-backups/, 0700) before it is rewritten.
// Usage: node scripts/backfill-muse-summary.mjs [--apply] [--data-dir <dir>] [--only <session-id>]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { build } from "esbuild";

const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
const apply = args.includes("--apply");
const only = flag("--only");
const dataDir = flag("--data-dir") ?? path.join(os.homedir(), "Library/Application Support/com.taisun.nexus");
const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");

const compiled = await build({ entryPoints: [path.join(root, "src-console/neuro/recordingSummary.ts")], bundle: true, write: false, format: "esm", platform: "node", tsconfig: path.join(root, "tsconfig.json") });
const { summarizeReplay } = await import(`data:text/javascript;base64,${Buffer.from(compiled.outputFiles[0].contents).toString("base64")}`);

const KEYS = ["avg_focus", "avg_relaxation", "calm_pct"];
const sessionsDir = path.join(dataDir, "sessions");
const backupDir = path.join(dataDir, "session-backups");
for (const name of fs.readdirSync(sessionsDir).filter((n) => /^muse_.*\.json$/.test(n)).sort()) {
  const file = path.join(sessionsDir, name);
  const raw = fs.readFileSync(file, "utf8");
  const doc = JSON.parse(raw);
  const session = doc.session ?? {};
  if (only && session.id !== only) continue;
  const summary = session.summary ?? {};
  if (KEYS.some((key) => summary[key] != null)) continue;
  const replayName = doc.data?.replay_file ?? session.file_path;
  if (typeof replayName !== "string" || replayName.includes("/") || replayName.includes("..")) { console.log(`${name}: skip (no replay)`); continue; }
  const replayPath = path.join(dataDir, "replays", replayName);
  if (!fs.existsSync(replayPath)) { console.log(`${name}: skip (replay missing)`); continue; }
  const result = summarizeReplay(fs.readFileSync(replayPath, "utf8"));
  if (KEYS.every((key) => result[key] == null)) { console.log(`${name}: skip (not enough valid data)`); continue; }
  console.log(`${name}: ${KEYS.map((k) => `${k}=${result[k]?.toFixed(1)}`).join(" ")}${apply ? "" : " (dry run)"}`);
  if (!apply) continue;
  fs.mkdirSync(backupDir, { recursive: true, mode: 0o700 });
  const backup = path.join(backupDir, `${name}.bak-${Date.now()}`);
  fs.writeFileSync(backup, raw, { mode: 0o600, flag: "wx" });
  doc.session.summary = { ...summary, ...Object.fromEntries(KEYS.map((k) => [k, result[k]])) };
  doc.data = { ...(doc.data ?? {}), recording_summary: { ...result, backfilled_at: new Date().toISOString() } };
  const tmp = `${file}.tmp-${process.pid}`;
  fs.writeFileSync(tmp, JSON.stringify(doc, null, 2), { mode: 0o600, flag: "wx" });
  fs.renameSync(tmp, file);
  console.log(`  backup: ${backup}`);
}
