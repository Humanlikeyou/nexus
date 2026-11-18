#!/usr/bin/env node
// Writes the Tauri updater manifest (latest.json) for one release (handover Phase 15 step 4).
// Refuses to run without a signing key and a real public key. It never reads, prints or stores the
// key: signing is done by `tauri signer sign`, which takes the key from the environment.
//
//   node scripts/make-update-manifest.mjs --bundle <Amara OS.app.tar.gz> --url <https://host/path/Amara OS.app.tar.gz> \
//        [--notes "text"] [--out dist-update/latest.json]
//
// Needs: TAURI_SIGNING_PRIVATE_KEY (or TAURI_SIGNING_PRIVATE_KEY_PATH) in the environment, and
// plugins.updater.pubkey in src-tauri/tauri.conf.json set to Taisun's real key (not the placeholder).
// The manifest host is whatever --url points at; it must match plugins.updater.endpoints' host.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const opt = (name) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
const die = (msg) => { console.error(`make-update-manifest: ${msg}`); process.exit(2); };

const confPath = process.env.NEXUS_TAURI_CONF || resolve(root, "src-tauri/tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));
const pubkey = conf.plugins?.updater?.pubkey;
const endpoints = conf.plugins?.updater?.endpoints ?? [];

if (!process.env.TAURI_SIGNING_PRIVATE_KEY && !process.env.TAURI_SIGNING_PRIVATE_KEY_PATH) {
  die("refusing: no signing key. Set TAURI_SIGNING_PRIVATE_KEY (or TAURI_SIGNING_PRIVATE_KEY_PATH) in the environment. The key is Taisun's and is never stored in this repo.");
}
if (!pubkey || pubkey === "PLACEHOLDER_NOT_CONFIGURED") die("refusing: plugins.updater.pubkey is still the placeholder.");
if (endpoints.length === 0) die("refusing: plugins.updater.endpoints is empty (no manifest host chosen).");

const bundle = opt("--bundle");
const url = opt("--url");
if (!bundle || !existsSync(bundle)) die("--bundle <file> is required and must exist.");
let u;
try { u = new URL(url ?? ""); } catch { die("--url must be an https URL."); }
if (u.protocol !== "https:" || u.username || u.password) die("--url must be https without credentials.");
const hosts = new Set(endpoints.map((e) => { try { return new URL(e).host; } catch { return ""; } }));
if (!hosts.has(u.host)) die(`--url host ${u.host} is not the configured manifest host.`);

// Sign with the Tauri CLI; the key stays in the environment. Output is the .sig file next to the bundle.
const r = spawnSync(resolve(root, "node_modules/.bin/tauri"), ["signer", "sign", bundle], { cwd: root, env: process.env, stdio: ["ignore", "pipe", "pipe"] });
const sigFile = `${bundle}.sig`;
if (r.status !== 0 || !existsSync(sigFile)) die("signing failed (check the key and its password variable).");
const signature = readFileSync(sigFile, "utf8").trim();

const version = conf.version;
const manifest = {
  version,
  notes: opt("--notes") ?? "",
  pub_date: new Date().toISOString(),
  platforms: { "darwin-aarch64": { signature, url: u.toString() } },
};
const out = resolve(opt("--out") ?? "dist-update/latest.json");
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(manifest, null, 2) + "\n");
console.log(`make-update-manifest: wrote ${out} for version ${version}`);
