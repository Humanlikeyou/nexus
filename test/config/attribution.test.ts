import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

// Stage G6 / decision 0033: NOOP's NOTICE, ATTRIBUTION.md and the PolyForm Noncommercial 1.0.0
// licence ship verbatim inside the app bundle. The repo holds the copies; tauri.conf.json maps them
// into Resources/third-party/noop/. About-screen copy is Design's.
const root = resolve(__dirname, "../..");
const DIR = "src-tauri/resources/third-party/noop";
const FILES = ["NOTICE", "ATTRIBUTION.md", "LICENSE"] as const;
// SHA-256 of the committed copies (taken from ~/projects/noop-lucid-current on 2026-11-18).
// Update together with the files when NOOP's text changes.
const SHA256: Record<(typeof FILES)[number], string> = {
  NOTICE: "0c95b9341258f322b1f608d23b77e02fc23fc9d6009013c8d1eb74679ad4c934",
  "ATTRIBUTION.md": "ef1a2c0ebf39a0d5bcdf7b892bfccd69a50366113eb3cdcbc154c4d94ac8530e",
  LICENSE: "6581224cd341c05996116bcb6c24e3b1fb36e03c3b0b3b78147a3ac17464542c",
};
const sha = (b: Buffer) => createHash("sha256").update(b).digest("hex");

describe("NOOP attribution in the bundle", () => {
  const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")) as { bundle: { resources?: Record<string, string> } };

  it.each(FILES)("%s is committed, byte-identical to its pinned digest, and mapped into the bundle", (f) => {
    const path = join(root, DIR, f);
    expect(existsSync(path)).toBe(true);
    expect(sha(readFileSync(path))).toBe(SHA256[f]);
    expect(conf.bundle.resources?.[`resources/third-party/noop/${f}`]).toBe(`third-party/noop/${f}`);
  });

  it("the licence is PolyForm Noncommercial 1.0.0 with the NoopApp copyright", () => {
    const lic = readFileSync(join(root, DIR, "LICENSE"), "utf8");
    expect(lic).toContain("PolyForm Noncommercial License 1.0.0");
    expect(lic).toContain("Copyright 2026 NoopApp");
  });

  // Where the source checkout exists, the committed copies must still be identical to it.
  const src = join(homedir(), "projects/noop-lucid-current");
  it.skipIf(!existsSync(join(src, "NOTICE")))("matches the NOOP source files byte for byte", () => {
    for (const f of FILES) expect(readFileSync(join(root, DIR, f)).equals(readFileSync(join(src, f)))).toBe(true);
  });
});
