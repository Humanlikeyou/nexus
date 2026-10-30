import { mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
// @ts-expect-error plain .mjs script without types
import { allowlisted, denyReason, main, plan, remoteProblems } from "../../scripts/kb-ingest.mjs";

const dirs: string[] = [];
afterEach(() => { for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true }); });

function repo(files: Record<string, string>): string {
  const root = mkdtempSync(join(tmpdir(), "kb-ingest-"));
  dirs.push(root);
  for (const [rel, body] of Object.entries(files)) {
    mkdirSync(join(root, rel, ".."), { recursive: true });
    writeFileSync(join(root, rel), body);
  }
  return root;
}
/** Every path with size and mtime: any write shows up as a difference. */
function tree(root: string): string[] {
  const out: string[] = [];
  const walk = (d: string) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, e.name);
      const st = statSync(p);
      out.push(`${p} ${st.size} ${st.mtimeMs}`);
      if (e.isDirectory()) walk(p);
    }
  };
  walk(root);
  return out.sort();
}
const run = async (argv: string[]) => {
  const lines: string[] = [];
  const code = await main(argv, (l: string) => lines.push(l));
  return { code, out: lines.join("\n") };
};
// Built at run time so this file never contains a literal key for check:secrets to flag.
const FAKE_SK = "sk-" + "FAKEtestKEY".repeat(3);
const FAKE_BEARER = "Authorization: Bearer " + "abcDEF123".repeat(4);
const localEnv = "OLLAMA_URL=http://host.docker.internal:11434\nWEAVIATE_URL_VERBA=http://weaviate:8080\nOPENAI_API_KEY=\n";

describe("kb-ingest: allowlist and denylist", () => {
  it("the allowlist covers the docs set and nothing else", () => {
    for (const p of ["README.md", "CLAUDE.md", "STAGES.md", "BUILD.md", "docs/a.md", "docs/x/y/z.md", "stack/SECURITY-NOTES.md", "stack/crew-service/README.md", "stack/n8n/workflows/notes.md"]) {
      expect(allowlisted(p), p).toBe(true);
    }
    for (const p of ["src/main.ts", "docs/a.txt", "stack/n8n/workflows/eYdwlwZKTMV89z2J.json", "stack/.env", "package.json"]) {
      expect(allowlisted(p), p).toBe(false);
    }
  });

  it.each([
    ["docs/secrets/keys.md", "secrets dir"],
    ["docs/sleep-log.md", "health data"],
    ["docs/noop/recovery.md", "health data"],
    ["docs/whoop-strain.md", "health data"],
    ["docs/muse-session.md", "neuro data"],
    ["docs/eeg-notes.md", "neuro data"],
    ["docs/logs/today.md", "log"],
    ["docs/backup-plan.md", "backup"],
    ["docs/credentials.md", "credentials"],
    ["docs/archive/old.md", "archive"],
  ])("denylist beats the allowlist: %s", (rel, reason) => {
    expect(allowlisted(rel)).toBe(true);
    expect(denyReason(rel)).toContain(reason);
    const root = repo({ [rel]: "# fine text\n", "README.md": "# ok\n" });
    const p = plan(root);
    expect(p.files.map((f: { rel: string }) => f.rel)).toEqual(["README.md"]);
    expect(p.skipped).toEqual([{ rel, reason: expect.stringContaining("denylist") }]);
  });
});

describe("kb-ingest: secret scan", () => {
  it.each([
    ["sk- key", `api notes ${FAKE_SK}\n`],
    ["bearer", `${FAKE_BEARER}\n`],
    ["jwt", `token eyJ${"a".repeat(12)}.eyJ${"b".repeat(12)}.${"c".repeat(12)}\n`],
    ["password", "db password = hunter2hunter2\n"],
  ])("a planted %s is skipped and listed by path only", async (_l, body) => {
    const root = repo({ "docs/leak.md": `# notes\n${body}`, "README.md": "# ok\n" });
    const p = plan(root);
    expect(p.files.map((f: { rel: string }) => f.rel)).toEqual(["README.md"]);
    expect(p.skipped[0]).toMatchObject({ rel: "docs/leak.md", reason: expect.stringContaining("secret scan") });
    const r = await run(["--repo", root, "--verba-env-file", join(root, "README.md")]);
    expect(r.out).toContain("docs/leak.md");
    expect(r.out).not.toContain(body.trim());
    expect(r.out).not.toContain(FAKE_SK);
  });
});

describe("kb-ingest: local only", () => {
  it.each([
    ["OPENAI_API_KEY=x-not-real\n", "OPENAI_API_KEY is set"],
    ["ANTHROPIC_API_KEY=x-not-real\n", "ANTHROPIC_API_KEY is set"],
    ["OLLAMA_URL=https://api.example.com\n", "OLLAMA_URL points at a remote host"],
    ["WEAVIATE_URL_VERBA=https://cluster.weaviate.cloud\n", "WEAVIATE_URL_VERBA points at a remote host"],
  ])("a remote provider aborts (%s), naming it without its value", async (env, problem) => {
    expect(remoteProblems(Object.fromEntries([env.trim().split("=")]))).toEqual([problem]);
    const root = repo({ "README.md": "# ok\n", "verba.env": localEnv + env });
    for (const mode of ["--dry-run", "--apply"]) {
      const r = await run([mode, "--repo", root, "--verba-env-file", join(root, "verba.env")]);
      expect(r.code).toBe(3);
      expect(r.out).toContain(problem);
      expect(r.out).not.toMatch(/x-not-real|api\.example\.com|weaviate\.cloud/);
      expect(r.out).not.toContain("Would ingest");
    }
  });

  it("empty key vars and local URLs pass", () => {
    expect(remoteProblems({ OPENAI_API_KEY: "", OLLAMA_URL: "http://host.docker.internal:11434", WEAVIATE_URL_VERBA: "http://weaviate:8080" })).toEqual([]);
  });
});

describe("kb-ingest: dry run", () => {
  it("is the default, lists files with bytes and totals, and writes nothing", async () => {
    const root = repo({
      "README.md": "# Nexus\n", "CLAUDE.md": "rules\n", "docs/a.md": "aaaa\n",
      "docs/sleep.md": "x\n", "stack/.env": "NEXUS_SECRET=whatever\n", "src/app.ts": "x\n", "verba.env": localEnv,
    });
    const before = tree(root);
    const r = await run(["--repo", root, "--verba-env-file", join(root, "verba.env")]);
    expect(r.code).toBe(0);
    expect(r.out).toContain("--dry-run");
    expect(r.out).toMatch(/README\.md {2}8 B/);
    expect(r.out).toContain("docs/sleep.md  (denylist: health data");
    expect(r.out).toContain("Totals: 3 files, 19 B");
    expect(r.out).toContain("Dry run: nothing written or sent.");
    expect(r.out).not.toContain("whatever");
    expect(tree(root)).toEqual(before);
  });

  it("--apply without a reachable verba env aborts before anything is sent", async () => {
    const root = repo({ "README.md": "# ok\n" });
    const r = await run(["--apply", "--repo", root, "--verba-env-file", join(root, "missing.env")]);
    expect(r.code).toBe(3);
    expect(r.out).toContain("ABORT");
  });
});
