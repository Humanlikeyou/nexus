#!/usr/bin/env node
/**
 * Copy NEXUS_SECRET from stack/.env into the app config's n8n_webhook_secret,
 * so memory.write / consolidation posts pass the n8n ingress X-Nexus-Secret
 * check. Only fills an empty value; never overwrites one that is set.
 *
 * The secret is never printed, logged or echoed. Output is one word:
 *   set        n8n_webhook_secret was empty and is now filled
 *   unchanged  it was already set (or NEXUS_SECRET is empty: exit 1)
 *
 * Usage:
 *   node scripts/sync-ingress-secret.mjs
 *   node scripts/sync-ingress-secret.mjs --config <config.json> --env <stack/.env>
 *
 * Defaults: $NEXUS_DATA_DIR/config.json, else
 * ~/Library/Application Support/com.taisun.nexus/config.json; stack/.env
 * next to this script's repo.
 */

import { readFileSync, writeFileSync, renameSync, chmodSync, unlinkSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const args = process.argv.slice(2);
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function getArg(flag, fallback) {
  const idx = args.indexOf(flag);
  return idx !== -1 && args[idx + 1] ? args[idx + 1] : fallback;
}

function fail(msg) {
  // Messages never include the secret.
  console.error(`sync-ingress-secret: ${msg}`);
  process.exit(1);
}

const dataDir = process.env.NEXUS_DATA_DIR || join(homedir(), "Library/Application Support/com.taisun.nexus");
const configPath = getArg("--config", join(dataDir, "config.json"));
const envPath = getArg("--env", join(repo, "stack/.env"));

/** Value of KEY in a dotenv file (handles quotes and `export `), or "". */
export function readEnvValue(text, key) {
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim().replace(/^export\s+/, "");
    if (!line.startsWith(`${key}=`)) continue;
    const v = line.slice(key.length + 1).trim();
    const q = v[0];
    if (q === '"' || q === "'") {
      const end = v.indexOf(q, 1);
      return end === -1 ? v.slice(1) : v.slice(1, end);
    }
    return v.replace(/\s+#.*$/, "");
  }
  return "";
}

if (!existsSync(envPath)) fail(`env file not found: ${envPath}`);
if (!existsSync(configPath)) fail(`config not found: ${configPath}`);

let config;
try {
  config = JSON.parse(readFileSync(configPath, "utf8"));
} catch {
  fail(`config is not valid JSON: ${configPath}`);
}
if (!config || typeof config !== "object" || Array.isArray(config)) fail("config is not a JSON object");

if (typeof config.n8n_webhook_secret === "string" && config.n8n_webhook_secret !== "") {
  console.log("unchanged");
  process.exit(0);
}

const secret = readEnvValue(readFileSync(envPath, "utf8"), "NEXUS_SECRET");
if (!secret) {
  console.log("unchanged");
  fail("NEXUS_SECRET is missing or empty in the env file");
}

config.n8n_webhook_secret = secret;
const tmp = join(dirname(configPath), `.config.json.${randomUUID()}.tmp`);
try {
  writeFileSync(tmp, JSON.stringify(config, null, 2), { mode: 0o600, flag: "wx" });
  chmodSync(tmp, 0o600);
  renameSync(tmp, configPath);
} catch (e) {
  try { unlinkSync(tmp); } catch { /* already gone */ }
  fail(`could not write config (${e.code ?? "error"})`);
}
console.log("set");
