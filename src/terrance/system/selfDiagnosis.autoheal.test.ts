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
  autoHeal, __resetAutoHealForTests, DECLINE_COOLDOWN_MS, DECLINE_COOLDOWN_KV_KEY, NEXUS_NOTICE_EVENT,
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
});
