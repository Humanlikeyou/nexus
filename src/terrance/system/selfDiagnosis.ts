/**
 * Terrance Self-Diagnosis + Auto-Healing
 *
 * Checks health of:
 *  - OpenClaw gateway (port 18789)
 *  - n8n (Docker container stack-n8n-1)
 *  - Ollama (port 11434)
 *
 * Auto-heal actions:
 *  - One batched restart_services approval for all down services
 *  - Alert via Telegram only when a restart fails after Allow
 */

import { invoke } from "@tauri-apps/api/core";
import type { DiagnosticReport, DiagnosticIssue, HealingAction } from "@/lib/types";
import { loadContext, saveContext } from "./continuity";
import { activityBus } from "./activityBus";
import { approvalOutcome } from "./approvalErrors";

// ─── Auto-heal state ──────────────────────────────────────────────────────────

/** After a Deny, don't prompt again for these services for 4 hours (persisted in kv). */
export const DECLINE_COOLDOWN_MS = 4 * 60 * 60 * 1000;
/** Deadline handed to restart_services: well inside the scheduler's 60 s health-job timeout. */
export const RESTART_DEADLINE_MS = 50_000;
export const DECLINE_COOLDOWN_KV_KEY = "autoheal.declinedUntil";
export const RESTART_DECLINED_NOTICE = "Restart declined";
/** Window event the console shell listens to for quiet, non-modal notices. */
export const NEXUS_NOTICE_EVENT = "nexus:notice";

export interface ServiceRestartResult {
  service: string;
  ok: boolean;
  error: string | null;
}

/**
 * - addressed: every service tried restarted OK (after Allow)
 * - declined:  user denied (or still inside a decline cooldown)
 * - awaiting:  prompt busy / expired / skipped this cycle; nothing ran
 * - failed:    real restart failure after Allow, or an unexpected error
 * - none:      nothing healable
 */
export type AutoHealStatus = "addressed" | "declined" | "awaiting" | "failed" | "none";

export interface AutoHealResult {
  status: AutoHealStatus;
  actions: HealingAction[];
}

/** Map a diagnostic issue to a restart_services id. */
function serviceIdFor(issue: DiagnosticIssue): string | null {
  switch (issue.service) {
    case "openclaw": return "openclaw";
    case "n8n": return "docker:stack-n8n-1";
    default: return null;
  }
}

let inFlight: Promise<AutoHealResult> | null = null;

async function loadDeclinedUntil(): Promise<Record<string, number>> {
  try {
    const raw = await invoke<string | null>("load_kv", { key: DECLINE_COOLDOWN_KV_KEY });
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed as Record<string, number> : {};
  } catch {
    return {};
  }
}

async function saveDeclinedUntil(map: Record<string, number>): Promise<void> {
  try {
    await invoke<void>("save_kv", { key: DECLINE_COOLDOWN_KV_KEY, value: JSON.stringify(map) });
  } catch (e) {
    console.error("[self-diagnosis] could not persist decline cooldown:", e);
  }
}

function quietNotice(message: string): void {
  if (typeof window === "undefined") return;
  try {
    window.dispatchEvent(new CustomEvent(NEXUS_NOTICE_EVENT, { detail: { message } }));
  } catch { /* no DOM */ }
}

async function telegramAlert(text: string): Promise<void> {
  try {
    await invoke<void>("send_telegram_message", { text });
  } catch {
    console.error("[self-diagnosis] Failed to send Telegram alert");
  }
}

/** Test-only: forget the in-flight cycle. */
export function __resetAutoHealForTests(): void {
  inFlight = null;
}

export async function runDiagnostics(): Promise<DiagnosticReport> {
  const issues: DiagnosticIssue[] = [];

  // Check OpenClaw gateway
  let openclawOnline = false;
  let openclawGateway = false;
  try {
    const health = await invoke<{
      status: string;
      gateway: boolean;
      rpc: boolean;
    }>("get_openclaw_health");
    openclawOnline = health.status !== "offline";
    openclawGateway = health.gateway;
    if (!openclawGateway) {
      issues.push({
        service: "openclaw",
        status: "down",
        message: "OpenClaw gateway not responding on port 18789",
        canAutoHeal: true,
      });
    } else if (health.status === "degraded") {
      issues.push({
        service: "openclaw",
        status: "degraded",
        message: "OpenClaw is degraded (check memory search or channels)",
        canAutoHeal: false,
      });
    }
  } catch {
    issues.push({
      service: "openclaw",
      status: "down",
      message: "Failed to check OpenClaw health",
      canAutoHeal: true,
    });
  }

  // Check n8n (Docker)
  let n8nReachable = false;
  try {
    const status = await invoke<string>("check_docker_service", { name: "stack-n8n-1" });
    n8nReachable = status === "running";
    if (!n8nReachable) {
      issues.push({
        service: "n8n",
        status: "down",
        message: `n8n Docker container status: ${status}`,
        canAutoHeal: true,
      });
    }
  } catch {
    issues.push({
      service: "n8n",
      status: "error",
      message: "Failed to check n8n container status",
      canAutoHeal: true,
    });
  }

  // Check Ollama
  let ollamaReachable = false;
  try {
    const status = await invoke<string>("check_service_port", { host: "127.0.0.1", port: 11434 });
    ollamaReachable = status === "open";
    if (!ollamaReachable) {
      issues.push({
        service: "ollama",
        status: "down",
        message: "Ollama not responding on port 11434",
        canAutoHeal: false,
      });
    }
  } catch {
    // Ollama check failed — not critical, it may not be running
  }

  // Update health check timestamp
  try {
    const ctx = await loadContext();
    await saveContext({ ...ctx, lastHealthCheckTimestamp: Date.now() });
  } catch {}

  return {
    timestamp: Date.now(),
    openclawOnline,
    openclawGateway,
    n8nReachable,
    ollamaReachable,
    diskSpaceOk: true, // Simplified — could add disk space check later
    issues,
  };
}

/**
 * Propose restarts for every down service in ONE native approval (restart_services).
 * Only one cycle is in flight at a time; concurrent calls get "awaiting".
 * Telegram alerts only for real failures after Allow.
 */
export function autoHeal(report: DiagnosticReport, now: number = Date.now()): Promise<AutoHealResult> {
  if (inFlight) return Promise.resolve({ status: "awaiting", actions: [] });
  inFlight = runAutoHeal(report, now).finally(() => { inFlight = null; });
  return inFlight;
}

async function runAutoHeal(report: DiagnosticReport, now: number): Promise<AutoHealResult> {
  const wanted = [...new Set(report.issues.filter((i) => i.canAutoHeal).map(serviceIdFor).filter((s): s is string => !!s))].slice(0, 8);
  if (wanted.length === 0) return { status: "none", actions: [] };

  const declinedUntil = await loadDeclinedUntil();
  const services = wanted.filter((s) => !(declinedUntil[s] > now));
  if (services.length === 0) {
    console.log("[self-diagnosis] restart recently declined; waiting out the 4h cooldown");
    return { status: "declined", actions: [] };
  }

  const taskId = activityBus.register("Auto-Heal", "auto_heal", { detail: `Proposing restart: ${services.join(", ")}` });
  let results: ServiceRestartResult[];
  try {
    results = await invoke<ServiceRestartResult[]>("restart_services", { services, deadlineMs: now + RESTART_DEADLINE_MS });
  } catch (e) {
    const outcome = approvalOutcome(e);
    switch (outcome) {
      case "declined": {
        const until = now + DECLINE_COOLDOWN_MS;
        for (const s of services) declinedUntil[s] = until;
        await saveDeclinedUntil(declinedUntil);
        console.log(`[self-diagnosis] restart declined for ${services.join(", ")}; next prompt after 4h`);
        quietNotice(RESTART_DECLINED_NOTICE);
        activityBus.complete(taskId, RESTART_DECLINED_NOTICE);
        return { status: "declined", actions: [] };
      }
      case "busy":
      case "expired":
        console.log(`[self-diagnosis] restart not run (${outcome}); will retry next cycle`);
        activityBus.complete(taskId, outcome === "busy" ? "Approval already open" : "Approval expired; nothing ran");
        return { status: "awaiting", actions: [] };
      case "refused":
      case "changed":
        console.error(`[self-diagnosis] BUG: restart_services refused the request: ${String(e)}`);
        activityBus.fail(taskId, String(e));
        return { status: "failed", actions: [] };
      default: {
        activityBus.fail(taskId, String(e));
        await telegramAlert(`⚠️ *Terrance Auto-Heal Failed*\n\nServices: \`${services.join(", ")}\`\nError: ${String(e)}\n\nManual intervention may be required.`);
        return { status: "failed", actions: [] };
      }
    }
  }

  const actions: HealingAction[] = results.map((r) => ({
    service: r.service,
    action: r.service.startsWith("docker:") ? "restart_docker" : "restart_launchagent",
    success: r.ok,
    message: r.ok ? "restarted" : (r.error ?? "restart failed"),
    timestamp: now,
  }));
  const failed = actions.filter((a) => !a.success);
  const allTriedOk = failed.length === 0 && services.every((s) => results.some((r) => r.service === s && r.ok));
  for (const f of failed) {
    await telegramAlert(`⚠️ *Terrance Auto-Heal Failed*\n\nService: \`${f.service}\`\nAction: ${f.action}\nError: ${f.message}\n\nManual intervention may be required.`);
  }
  if (allTriedOk) activityBus.complete(taskId, `${actions.length} service(s) restarted`);
  else activityBus.fail(taskId, `${failed.length}/${actions.length} restart(s) failed`);

  try {
    await invoke<void>("store_session_log", {
      entry: {
        id: `heal-${Date.now()}`,
        timestamp: Date.now(),
        messageCount: 0,
        summary: `[Auto-Heal Report]\n${actions.map((a) => `${a.service}: ${a.action} → ${a.success ? "OK" : "FAILED"} (${a.message})`).join("\n")}`,
        provider: "system",
        model: "self-diagnosis",
      },
    });
  } catch {}

  return { status: allTriedOk ? "addressed" : "failed", actions };
}
