import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { entropy, forbiddenEnvFile, parseAllowlist, scanRepo, scanText } from "../../scripts/check-secrets.mjs";

// Fixtures are assembled at runtime so this tracked file never contains a literal that the
// gate itself would flag. None of these are real credentials.
const j = (...parts: string[]) => parts.join("");
const RAND = "Q7vK2mXp9LrT4wZb8NcY6hJd3FgS5aE1"; // 32 chars, high entropy
const FIX = {
  openai: j("sk", "-proj-", RAND),
  anthropic: j("sk", "-ant-api03-", RAND),
  github: j("gh", "p_", RAND, "AbCd"),
  githubPat: j("github", "_pat_", RAND),
  slack: j("xo", "xb-", "1234567890-", RAND),
  aws: j("AK", "IA", "QWERTYUIOPASDFGH"),
  telegram: j("123456789", ":", "AAH", RAND),
  pem: j("-----BEGIN ", "RSA PRIVATE", " KEY-----"),
};

const scan = (text: string, allow = "") => scanText("fixture.txt", text, parseAllowlist(allow));
const patterns = (text: string, allow = "") => scan(text, allow).map((f) => f.pattern);

describe("check:secrets patterns (hits)", () => {
  it.each([
    ["openai-key", `OPENAI=${FIX.openai}`],
    ["anthropic-key", `key: "${FIX.anthropic}"`],
    ["github-token", `token ${FIX.github}`],
    ["github-pat", FIX.githubPat],
    ["slack-token", `hook=${FIX.slack}`],
    ["aws-access-key", `aws ${FIX.aws}`],
    ["telegram-bot-token", `bot ${FIX.telegram}`],
    ["pem-private-key", FIX.pem],
    ["assigned-high-entropy", `CREW_SERVICE_TOKEN=${RAND}`],
    ["assigned-high-entropy", `"db_password": "${RAND}"`],
    ["assigned-high-entropy", `export N8N_SECRET='${RAND}'`],
  ])("%s", (name, line) => {
    expect(patterns(line)).toContain(name);
  });

  it("anthropic keys are not double-reported as openai keys", () => {
    expect(patterns(FIX.anthropic)).toEqual(["anthropic-key"]);
  });

  it("reports file:line and pattern only, never the matched text", () => {
    const findings = scanText("a/b.env.sh", `ok\nX_TOKEN=${RAND}\n${FIX.telegram}`);
    expect(findings).toEqual([
      { file: "a/b.env.sh", line: 2, pattern: "assigned-high-entropy" },
      { file: "a/b.env.sh", line: 3, pattern: "telegram-bot-token" },
    ]);
    expect(JSON.stringify(findings)).not.toContain(RAND);
  });
});

describe("check:secrets patterns (misses)", () => {
  it.each([
    ["short sk- words", "use scikit sk-learn and risk-free desk-top"],
    ["placeholders in .env.example style", "NEXUS_SECRET=\nTELEGRAM_BOT_TOKEN=changeme"],
    ["low-entropy assigned value", `API_KEY=${"a".repeat(40)}`],
    ["short assigned value", "API_KEY=abc123"],
    ["dotted storage-key identifier", `const AUTOSAVE_KEY = "nexus.chatHistory.lastAutosaveSignature";`],
    ["env reference, not a value", "password: process.env.POSTGRES_PASSWORD"],
    ["public PEM", j("-----BEGIN ", "PUBLIC KEY-----")],
    ["timestamp:hash that is not a bot token", "1793322975:abc"],
    ["sha512 integrity hash", `"integrity": "sha512-${RAND}"`],
  ])("%s", (_label, text) => {
    expect(scan(text)).toEqual([]);
  });

  it("entropy: random keys score high, repeats score zero", () => {
    expect(entropy(RAND)).toBeGreaterThan(3.5);
    expect(entropy("aaaa")).toBe(0);
  });
});

describe("check:secrets allowlist", () => {
  const fake = j("FAKE_", "TOKEN", "_NOT_REAL");

  it("allows a known-fake fixture by glob", () => {
    const line = `API_TOKEN=${fake}_${RAND}`;
    expect(patterns(line)).toEqual(["assigned-high-entropy"]);
    expect(patterns(line, "FAKE_*_NOT_REAL")).toEqual([]);
  });

  it("allows only what the glob covers", () => {
    const allow = "# comment\n\nFAKE_*_NOT_REAL   # trailing comment\n";
    expect(patterns(`${fake} ${FIX.telegram}`, allow)).toEqual(["telegram-bot-token"]);
  });

  it("glob characters are literal except *", () => {
    const [re] = parseAllowlist("a.b*");
    expect(re.test("a.bXYZ")).toBe(true);
    expect(re.test("aXb")).toBe(false);
  });
});

describe("check:secrets tracked env files", () => {
  it.each([
    [".env", true], ["stack/.env", true], [".env.local", true], ["stack/.env.production", true],
    [".env.example", false], ["stack/.env.example", false], [".env.test", false],
    ["docs/env.md", false], ["src/.envrc-notes.txt", false],
  ])("%s → forbidden %s", (file, expected) => {
    expect(forbiddenEnvFile(file)).toBe(expected);
  });
});

describe("check:secrets scanRepo (temp git repo)", () => {
  const git = (cwd: string, ...args: string[]) => execFileSync("git", args, { cwd, stdio: "ignore" });

  it("scans tracked files only, flags a tracked .env, honours .secrets-allow", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "check-secrets-"));
    try {
      git(dir, "init", "-q");
      fs.writeFileSync(path.join(dir, ".env"), "PLAIN=1\n");
      fs.writeFileSync(path.join(dir, "leak.ts"), `ok\nconst t = "${FIX.telegram}";\n`);
      fs.writeFileSync(path.join(dir, "fixture.ts"), `X_TOKEN=${j("FAKE_", "X", "_NOT_REAL")}_${RAND}\n`);
      fs.writeFileSync(path.join(dir, "untracked.ts"), `${FIX.aws}\n`);
      fs.writeFileSync(path.join(dir, ".secrets-allow"), "FAKE_*_NOT_REAL\n");
      git(dir, "add", ".env", "leak.ts", "fixture.ts", ".secrets-allow");
      const { files, findings } = scanRepo(dir);
      expect(files).toBe(4);
      expect(findings).toEqual([
        { file: ".env", line: 0, pattern: "tracked-env-file" },
        { file: "leak.ts", line: 2, pattern: "telegram-bot-token" },
      ]);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
