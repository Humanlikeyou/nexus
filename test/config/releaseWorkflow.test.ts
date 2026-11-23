import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

// No YAML dependency: steps are "      - " items; a `run: |` block is the lines indented deeper.
const yml = readFileSync(resolve(__dirname, "../../.github/workflows/release.yml"), "utf8");
type Step = { name?: string; uses?: string; run?: string };
const steps: Step[] = yml.split(/\n      - /).slice(1).map((chunk) => {
  const lines = chunk.split("\n");
  const field = (k: string) => lines.map((l) => l.trim()).find((l) => l.startsWith(`${k}: `))?.slice(k.length + 2).replace(/ #.*$/, "");
  const runAt = lines.findIndex((l) => l.trim() === "run: |");
  const run = runAt < 0 ? undefined : lines.slice(runAt + 1).filter((l) => l.startsWith("          ") || l.trim() === "").map((l) => l.slice(10)).join("\n");
  return { name: field("name") ?? (lines[0].startsWith("name: ") ? lines[0].slice(6) : undefined), uses: lines[0].startsWith("uses: ") ? lines[0].slice(6).replace(/ #.*$/, "") : field("uses"), run };
});

describe("release workflow", () => {
  it("pins every third-party action that receives secrets to a full commit SHA", () => {
    const tauri = steps.find((s) => s.uses?.startsWith("tauri-apps/tauri-action@"));
    expect(tauri?.uses).toMatch(/^tauri-apps\/tauri-action@[0-9a-f]{40}$/);
  });

  // Runs the real step script with the secrets as env vars.
  const keyStep = steps.find((s) => s.name === "Write App Store Connect API key")!;
  function runKeyStep(env: Record<string, string>) {
    const runnerTemp = mkdtempSync(join(tmpdir(), "rel-"));
    const githubEnv = join(runnerTemp, "github_env");
    try {
      execFileSync("bash", ["-e", "-c", keyStep.run!], {
        env: { PATH: process.env.PATH ?? "", RUNNER_TEMP: runnerTemp, GITHUB_ENV: githubEnv, ...env },
        stdio: "pipe",
      });
      return { code: 0, files: readdirSync(runnerTemp) };
    } catch (e) {
      return { code: (e as { status: number }).status, files: readdirSync(runnerTemp) };
    }
  }

  it("fails the release when APPLE_API_KEY is set but APPLE_API_KEY_P8 is missing", () => {
    expect(runKeyStep({ APPLE_API_KEY_ID: "ABC123", APPLE_API_KEY_P8: "" }).code).toBe(1);
  });
  it("builds unsigned only when both are unset", () => {
    expect(runKeyStep({ APPLE_API_KEY_ID: "", APPLE_API_KEY_P8: "" }).code).toBe(0);
  });
  it("writes the key file when both are set", () => {
    const r = runKeyStep({ APPLE_API_KEY_ID: "ABC123", APPLE_API_KEY_P8: "FAKE_P8_NOT_REAL" });
    expect(r.code).toBe(0);
    expect(r.files).toContain("AuthKey_ABC123.p8");
  });
});
