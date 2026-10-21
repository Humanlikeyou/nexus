// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));
vi.mock("./continuity", () => ({ loadContext: vi.fn(), saveContext: vi.fn() }));
vi.mock("./activityBus", () => ({
  activityBus: { register: vi.fn(() => "t"), complete: vi.fn(), fail: vi.fn(), update: vi.fn(), prune: vi.fn() },
}));

import {
  autoHeal, __resetAutoHealForTests, loadDeclinedUntil, DECLINE_COOLDOWN_MS, DECLINE_COOLDOWN_KV_KEY, NEXUS_NOTICE_EVENT,
  RESTART_DEADLINE_MS, RESTART_DECLINED_NOTICE,
} from "./selfDiagnosis";
import { approvalOutcome } from "./approvalErrors";
import type { DiagnosticReport } from "@/lib/types";

const NOW = 1_800_000_000_000;
const issue = (service: string) => ({ service, status: "down", message: "x", canAutoHeal: true }) as any;
const report = (...services: string[]): DiagnosticReport => ({ issues: services.map(issue) } as any);

let kv: Record<string, string>;
let restart: (args: any) => Promise<unknown>;
const calls = (cmd: string) => invoke.mock.calls.filter(([c]) => c === cmd);

beforeEach(() => {
  __resetAutoHealForTests();
  invoke.mockReset();
  kv = {};
  restart = async (a) => a.services.map((s: string) => ({ service: s, ok: true, error: null }));
  invoke.mockImplementation(async (cmd: string, args: any) => {
    switch (cmd) {
      case "load_kv": return kv[args.key] ?? null;
      case "save_kv": kv[args.key] = args.value; return;
      case "restart_services": return restart(args);
      default: return undefined;
    }
  });
});

describe("approvalOutcome prefixes", () => {
  it("matches declined:/busy:/expired:/refused: by prefix only", () => {
    expect(approvalOutcome("declined: Denied in the native approval dialog.")).toBe("declined");
    expect(approvalOutcome("busy: An approval dialog is already open.")).toBe("busy");
    expect(approvalOutcome(new Error("expired: deadline passed"))).toBe("expired");
    expect(approvalOutcome("refused: too large")).toBe("refused");
    expect(approvalOutcome("Denied in the native approval dialog. Nothing was changed.")).toBe("error");
    expect(approvalOutcome("docker failed: declined: nope")).toBe("error");
  });
});

describe("autoHeal (batched restart_services)", () => {
  it("one prompt for several services, deadline inside the 60 s health timeout", async () => {
    const r = await autoHeal(report("openclaw", "n8n", "ollama"), NOW);
    const rs = calls("restart_services");
    expect(rs).toHaveLength(1);
    expect(rs[0][1]).toEqual({ services: ["openclaw", "docker:stack-n8n-1"], deadlineMs: NOW + RESTART_DEADLINE_MS });
    expect(RESTART_DEADLINE_MS).toBeLessThan(60_000);
    expect(r.status).toBe("addressed");
    expect(calls("send_telegram_message")).toHaveLength(0);
  });

  it("skips the call entirely when nothing healable is down", async () => {
    const r = await autoHeal(report("ollama"), NOW);
    expect(calls("restart_services")).toHaveLength(0);
    expect(r.status).toBe("none");
  });

  it("declined: no Telegram, quiet notice, 4h cooldown persisted to kv", async () => {
    restart = async () => { throw "declined: Denied in the native approval dialog. Nothing was changed."; };
    const notices: string[] = [];
    window.addEventListener(NEXUS_NOTICE_EVENT, (e) => notices.push((e as CustomEvent).detail.message));
    const r = await autoHeal(report("openclaw", "n8n"), NOW);
    expect(r.status).toBe("declined");
    expect(calls("send_telegram_message")).toHaveLength(0);
    expect(notices).toContain(RESTART_DECLINED_NOTICE);
    expect(JSON.parse(kv[DECLINE_COOLDOWN_KV_KEY])).toEqual({
      openclaw: NOW + DECLINE_COOLDOWN_MS, "docker:stack-n8n-1": NOW + DECLINE_COOLDOWN_MS,
    });
  });

  it("persisted cooldown is reloaded (e.g. after relaunch) and expires after 4h", async () => {
    kv[DECLINE_COOLDOWN_KV_KEY] = JSON.stringify({ openclaw: NOW + DECLINE_COOLDOWN_MS });
    const during = await autoHeal(report("openclaw"), NOW + 60 * 60_000);
    expect(during.status).toBe("declined");
    expect(calls("restart_services")).toHaveLength(0);
    const after = await autoHeal(report("openclaw"), NOW + DECLINE_COOLDOWN_MS + 1);
    expect(after.status).toBe("addressed");
    expect(calls("restart_services")).toHaveLength(1);
  });

  it("busy: skip this cycle, no cooldown, no alert, status awaiting", async () => {
    restart = async () => { throw "busy: An approval dialog for this operation is already open."; };
    const r = await autoHeal(report("openclaw"), NOW);
    expect(r.status).toBe("awaiting");
    expect(kv[DECLINE_COOLDOWN_KV_KEY]).toBeUndefined();
    expect(calls("send_telegram_message")).toHaveLength(0);
  });

  it("expired: (late Allow after the deadline) nothing ran, no alert, status awaiting", async () => {
    restart = async () => { throw "expired: The restart request's deadline passed; nothing was run."; };
    const r = await autoHeal(report("openclaw"), NOW);
    expect(r.status).toBe("awaiting");
    expect(r.actions).toHaveLength(0);
    expect(calls("send_telegram_message")).toHaveLength(0);
  });

  it("refused: logged as a bug, no Telegram", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    restart = async () => { throw "refused: too large to show"; };
    const r = await autoHeal(report("openclaw"), NOW);
    expect(r.status).toBe("failed");
    expect(calls("send_telegram_message")).toHaveLength(0);
    expect(err.mock.calls.some((c) => String(c[0]).includes("BUG"))).toBe(true);
  });

  it("other errors are real failures and alert via Telegram", async () => {
    restart = async () => { throw "docker daemon unreachable"; };
    const r = await autoHeal(report("n8n"), NOW);
    expect(r.status).toBe("failed");
    expect(calls("send_telegram_message")).toHaveLength(1);
  });

  it("per-service ok:false after Allow alerts only for that service; not 'addressed'", async () => {
    restart = async () => [
      { service: "openclaw", ok: true, error: null },
      { service: "docker:stack-n8n-1", ok: false, error: "exit 1" },
    ];
    const r = await autoHeal(report("openclaw", "n8n"), NOW);
    expect(r.status).toBe("failed");
    const tg = calls("send_telegram_message");
    expect(tg).toHaveLength(1);
    expect(String(tg[0][1].text)).toContain("docker:stack-n8n-1");
  });

  it("dedupes concurrent cycles: only one prompt in flight", async () => {
    let release!: () => void;
    restart = (a) => new Promise((res) => { release = () => res(a.services.map((s: string) => ({ service: s, ok: true, error: null }))); });
    const first = autoHeal(report("openclaw"), NOW);
    const second = await autoHeal(report("openclaw"), NOW);
    expect(second.status).toBe("awaiting");
    await vi.waitFor(() => expect(calls("restart_services")).toHaveLength(1));
    release();
    expect((await first).status).toBe("addressed");
  });

  it("per-service expired: mid-batch is awaiting/not-run: no Telegram, no cooldown", async () => {
    restart = async () => [
      { service: "openclaw", ok: true, error: null },
      { service: "docker:stack-n8n-1", ok: false, error: "expired: deadline passed before this service ran" },
    ];
    const r = await autoHeal(report("openclaw", "n8n"), NOW);
    expect(r.status).toBe("awaiting");
    expect(calls("send_telegram_message")).toHaveLength(0);
    expect(kv[DECLINE_COOLDOWN_KV_KEY]).toBeUndefined();
    expect(r.actions.map((a) => a.service)).toEqual(["openclaw"]);
  });

  it("Telegram never contains raw error text (whole-call or per-service)", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const secret = "token=abc123 /Users/me/.secret stderr dump";
    restart = async () => { throw secret; };
    await autoHeal(report("openclaw"), NOW);
    restart = async () => [{ service: "openclaw", ok: false, error: secret }];
    await autoHeal(report("openclaw"), NOW);
    const texts = calls("send_telegram_message").map((c) => String(c[1].text));
    expect(texts).toEqual(["Auto-heal: restart failed (openclaw)", "Auto-heal: restart failed (openclaw)"]);
    expect(texts.join(" ")).not.toContain("abc123");
  });
});

describe("decline cooldown fails closed", () => {
  const CAP = NOW + DECLINE_COOLDOWN_MS;
  const cands = ["openclaw", "docker:stack-n8n-1"];
  const load = async (value: string | null | Error) => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd !== "load_kv") return undefined;
      if (value instanceof Error) throw value;
      return value;
    });
    vi.spyOn(console, "error").mockImplementation(() => {});
    return loadDeclinedUntil(cands, NOW);
  };

  it("missing key means nothing is cooling down", async () => {
    expect(await load(null)).toEqual({});
  });
  it.each([
    ["array", "[1,2]"], ["null", "null"], ["number", "42"], ["string", '"x"'],
  ])("non-object value (%s) cools every candidate until now+4h", async (_n, raw) => {
    expect(await load(raw)).toEqual({ openclaw: CAP, "docker:stack-n8n-1": CAP });
  });
  it("parse error or load error cools every candidate until now+4h", async () => {
    expect(await load("{not json")).toEqual({ openclaw: CAP, "docker:stack-n8n-1": CAP });
    expect(await load(new Error("io"))).toEqual({ openclaw: CAP, "docker:stack-n8n-1": CAP });
  });
  it("malformed per-service values (NaN/null/string/object) mean now+4h", async () => {
    for (const bad of ['"soon"', "null", "{}", '"NaN"']) {
      expect(await load(`{"openclaw": ${bad}}`)).toEqual({ openclaw: CAP });
    }
  });
  it("far-future values are clamped to now+4h; past values pass through", async () => {
    expect(await load(JSON.stringify({ openclaw: 9e15, "docker:stack-n8n-1": NOW - 1 })))
      .toEqual({ openclaw: CAP, "docker:stack-n8n-1": NOW - 1 });
  });
  it("a malformed store blocks the prompt (no restart_services call)", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    kv[DECLINE_COOLDOWN_KV_KEY] = "[]";
    const r = await autoHeal(report("openclaw"), NOW);
    expect(r.status).toBe("declined");
    expect(calls("restart_services")).toHaveLength(0);
  });
});

describe("cooldown resilience", () => {
  it("save_kv failing after a Deny still blocks the next cycle (in-memory cooldown)", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "save_kv") throw new Error("disk full");
      return base(cmd, args);
    });
    restart = async () => { throw "declined: Denied"; };
    expect((await autoHeal(report("openclaw"), NOW)).status).toBe("declined");
    expect(kv[DECLINE_COOLDOWN_KV_KEY]).toBeUndefined();
    const next = await autoHeal(report("openclaw"), NOW + 5 * 60_000);
    expect(next.status).toBe("declined");
    expect(calls("restart_services")).toHaveLength(1);
  });

  it("corrupted kv is repaired once: no prompt for 4h, then a prompt", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    kv[DECLINE_COOLDOWN_KV_KEY] = "{garbage";
    expect((await autoHeal(report("openclaw"), NOW)).status).toBe("declined");
    expect(JSON.parse(kv[DECLINE_COOLDOWN_KV_KEY])).toEqual({ openclaw: NOW + DECLINE_COOLDOWN_MS });
    expect(warn.mock.calls.some((c) => String(c[0]).includes("repairing"))).toBe(true);
    expect((await autoHeal(report("openclaw"), NOW + 3 * 60 * 60_000)).status).toBe("declined");
    expect(calls("restart_services")).toHaveLength(0);
    expect((await autoHeal(report("openclaw"), NOW + DECLINE_COOLDOWN_MS + 1)).status).toBe("addressed");
    expect(calls("restart_services")).toHaveLength(1);
  });

  it("a failed repair save is still bounded to 4h by the in-memory map", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const base = invoke.getMockImplementation()!;
    invoke.mockImplementation(async (cmd: string, args: any) => {
      if (cmd === "load_kv") return "[]"; // stays corrupted: repair can't be written
      if (cmd === "save_kv") throw new Error("read-only");
      return base(cmd, args);
    });
    expect((await autoHeal(report("openclaw"), NOW)).status).toBe("declined");
    expect((await autoHeal(report("openclaw"), NOW + 60 * 60_000)).status).toBe("declined");
    expect(calls("restart_services")).toHaveLength(0);
    // kv is still corrupted, but the in-memory deadline from the first repair (NOW+4h) holds.
    expect((await autoHeal(report("openclaw"), NOW + DECLINE_COOLDOWN_MS + 1)).status).toBe("addressed");
    expect(calls("restart_services")).toHaveLength(1);
  });
});
