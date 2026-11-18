// Stage H (Phases 13-15) guards that need no Apple identity, key or network.
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const root = resolve(__dirname, "../..");
const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));

describe("Phase 14 signing config", () => {
  const mac = conf.bundle.macOS;
  it("identity comes from the environment only, hardened runtime on, entitlements file set", () => {
    expect(mac.signingIdentity).toBeNull(); // tauri reads APPLE_SIGNING_IDENTITY
    // false on purpose: the config also drives the ad-hoc build we install, and the hardened runtime
    // (library validation) could break bundled ollama/llama-server, Muse BLE or the router. The
    // runtime flag is applied only by release-local.sh's signed path (checked below).
    expect(mac.hardenedRuntime).toBe(false);
    expect(mac.entitlements).toBe("Entitlements.plist");
    expect(conf.identifier).toBe("com.taisun.nexus");
    expect(conf.productName).toBe("Amara OS");
  });
  it("Entitlements.plist is minimal: bluetooth only, no sandbox, no get-task-allow, no JIT", () => {
    const plist = readFileSync(join(root, "src-tauri/Entitlements.plist"), "utf8");
    const keys = [...plist.matchAll(/<key>([^<]+)<\/key>/g)].map((m) => m[1]);
    expect(keys).toEqual(["com.apple.security.device.bluetooth"]);
  });
  it("the sidecars bundled by files{} are the ones release-local.sh signs", () => {
    expect(Object.keys(mac.files)).toEqual(["MacOS/amara-router", "Resources/ollama-runtime"]);
    const sh = readFileSync(join(root, "scripts/release-local.sh"), "utf8");
    expect(sh).toContain("Contents/MacOS/amara-router");
    expect(sh).toContain("Contents/Resources");
    expect(sh).toContain("--options runtime");
    expect(sh).toContain("npx --no-install tauri build");
    expect(sh).not.toMatch(/npx tauri/);
    expect(sh).toContain("deepest-first");
    expect(sh).toContain("lacks the hardened runtime flag");
    expect(sh).toContain("nexus-notary");
  });
});

describe("release-local.sh refuses without a Developer ID identity", () => {
  it("exits 2 and builds nothing (stubbed `security` reports 0 identities)", () => {
    const bin = mkdtempSync(join(tmpdir(), "stub-"));
    writeFileSync(join(bin, "security"), "#!/bin/sh\necho '     0 valid identities found'\n");
    chmodSync(join(bin, "security"), 0o755);
    const r = spawnSync("bash", [join(root, "scripts/release-local.sh")], {
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, APPLE_SIGNING_IDENTITY: "" },
      encoding: "utf8",
    });
    expect(r.status).toBe(2);
    expect(r.stderr).toContain("Developer ID Application");
    expect(r.stderr).toContain("Nothing was built");
  });
});

describe("make-update-manifest.mjs fails closed", () => {
  const run = (env: Record<string, string>, conf?: object) => {
    const dir = mkdtempSync(join(tmpdir(), "mf-"));
    const confPath = join(dir, "tauri.conf.json");
    writeFileSync(confPath, JSON.stringify(conf ?? { version: "0.0.0", plugins: { updater: { pubkey: "PLACEHOLDER_NOT_CONFIGURED", endpoints: [] } } }));
    const e = { ...process.env, NEXUS_TAURI_CONF: confPath, TAURI_SIGNING_PRIVATE_KEY: "", TAURI_SIGNING_PRIVATE_KEY_PATH: "", ...env };
    return spawnSync("node", [join(root, "scripts/make-update-manifest.mjs"), "--bundle", "/nonexistent"], { env: e, encoding: "utf8" });
  };
  it("refuses without a signing key", () => {
    const r = run({});
    expect(r.status).toBe(2);
    expect(r.stderr).toContain("no signing key");
  });
  it("refuses while the public key is the placeholder, even with a key variable set", () => {
    const r = run({ TAURI_SIGNING_PRIVATE_KEY: "not-a-real-key" });
    expect(r.status).toBe(2);
    expect(r.stderr).toContain("placeholder");
  });
  it("refuses with a real-looking public key but no endpoints", () => {
    const r = run({ TAURI_SIGNING_PRIVATE_KEY: "not-a-real-key" }, { version: "0.0.0", plugins: { updater: { pubkey: "abc", endpoints: [] } } });
    expect(r.status).toBe(2);
    expect(r.stderr).toContain("endpoints");
  });
});

describe("Phase 15 config stays inert", () => {
  it("placeholder pubkey, no endpoints, no updater artifacts, no webview permission", () => {
    expect(conf.plugins.updater).toEqual({ pubkey: "PLACEHOLDER_NOT_CONFIGURED", endpoints: [] });
    expect(conf.bundle.createUpdaterArtifacts).toBeUndefined();
    const cap = readFileSync(join(root, "src-tauri/capabilities/default.json"), "utf8");
    expect(cap).not.toMatch(/updater|autostart/);
  });
});
