/**
 * Amara Self-Diagnosis + Auto-Healing
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
import { buildRouterConfig } from "@/lib/llm-router";

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

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v) && Object.getPrototypeOf(v) === Object.prototype;
}

/**
 * Load the persisted decline cooldowns for `candidates`, failing CLOSED:
 * a load/parse error or a non-object value cools every candidate until
 * now+4h; a present non-finite value cools that service until now+4h; every
 * value is clamped to at most now+4h so a far-future value can't suppress a
 * service forever. Absent entries mean "not declined".
 */
export async function loadDeclinedUntil(candidates: string[], now: number): Promise<Record<string, number>> {
  return (await readKvCooldowns(candidates, now)).values;
}

async function readKvCooldowns(candidates: string[], now: number): Promise<{ values: Record<string, number>; corrupted: boolean }> {
  const cap = now + DECLINE_COOLDOWN_MS;
  const all = () => ({ values: Object.fromEntries(candidates.map((s) => [s, cap])), corrupted: true });
  let parsed: unknown;
  try {
    const raw = await invoke<string | null>("load_kv", { key: DECLINE_COOLDOWN_KV_KEY });
    if (raw == null) return { values: {}, corrupted: false };
    parsed = JSON.parse(raw);
  } catch (e) {
    console.error("[self-diagnosis] decline cooldown unreadable; failing closed:", e);
    return all();
  }
  if (!isPlainObject(parsed)) {
    console.error("[self-diagnosis] decline cooldown malformed; failing closed");
    return all();
  }
  const out: Record<string, number> = {};
  for (const s of candidates) {
    if (!Object.prototype.hasOwnProperty.call(parsed, s)) continue;
    const v = parsed[s];
    out[s] = typeof v === "number" && Number.isFinite(v) ? Math.min(v, cap) : cap;
  }
  return { values: out, corrupted: false };
}

/**
 * In-memory mirror of the decline cooldowns. Merged with kv (max per service,
 * clamped to now+4h) on every check, so a failed save_kv after a Deny or a
 * corrupted kv value still bounds prompting to the 4h window for this run.
 */
const memDeclinedUntil: Record<string, number> = {};
const repairedThisRun = new Set<string>();

/** Effective cooldowns for `candidates`; repairs a corrupted kv value once. */
async function currentCooldowns(candidates: string[], now: number): Promise<Record<string, number>> {
  const cap = now + DECLINE_COOLDOWN_MS;
  const { values, corrupted } = await readKvCooldowns(candidates, now);
  if (corrupted) {
    // Fail closed once, then repair: a clean, dated map so the block ends after 4h
    // instead of recomputing now+4h forever.
    // Services already repaired this run keep their original deadline, so a
    // repair that can't be saved still ends after 4h instead of sliding forever.
    const clean: Record<string, number> = {};
    for (const s of candidates) {
      if (!repairedThisRun.has(s)) {
        memDeclinedUntil[s] = Math.max(memDeclinedUntil[s] ?? 0, cap);
        repairedThisRun.add(s);
      }
      clean[s] = memDeclinedUntil[s];
    }
    console.warn(`[self-diagnosis] repairing corrupted decline cooldown in kv; blocking ${candidates.join(", ")} until ${new Date(cap).toISOString()}`);
    await saveDeclinedUntil(clean);
  }
  const merged: Record<string, number> = {};
  for (const s of candidates) {
    // A corrupted read yields synthetic now+4h values; rely on the dated in-memory map instead.
    const v = Math.max(corrupted ? 0 : values[s] ?? 0, memDeclinedUntil[s] ?? 0);
    if (v > 0) merged[s] = Math.min(v, cap);
  }
  return merged;
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
  for (const k of Object.keys(memDeclinedUntil)) delete memDeclinedUntil[k];
  repairedThisRun.clear();
}

export async function runDiagnostics(): Promise<DiagnosticReport> {
  const issues: DiagnosticIssue[] = [];

  // Local mode (the router default): the router only calls Ollama, so a down OpenClaw is not an
  // issue and is never restarted (10-28/10-29: every health check failed an OpenClaw restart).
  // An unreadable config falls back to the router default, local.
  let localMode = true;
  try {
    localMode = buildRouterConfig((await invoke<Record<string, unknown> | null>("get_config")) ?? {}).mode === "local";
  } catch { /* router default */ }

  // Check OpenClaw gateway
  let openclawOnline = false;
  let openclawGateway = false;
  let openclawInstalled = true;
  try {
    const health = await invoke<{
      status: string;
      gateway: boolean;
      rpc: boolean;
      installed?: boolean;
    }>("get_openclaw_health");
    openclawOnline = health.status !== "offline";
    openclawGateway = health.gateway;
    // Nothing listening and no launchd job: not installed, so nothing to restart or report as
    // down (live check 2026-10-29: every health check failed a launchctl kickstart).
    openclawInstalled = openclawGateway || health.installed !== false;
    if (!openclawInstalled) {
      console.log("[self-diagnosis] OpenClaw not installed (no listener on 18789, no launchd job); skipping");
    } else if (!openclawGateway && localMode) {
      console.log("[self-diagnosis] OpenClaw gateway down but compute_mode is local; not restarting");
    } else if (!openclawGateway) {
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
    if (!localMode) {
      issues.push({
        service: "openclaw",
        status: "down",
        message: "Failed to check OpenClaw health",
        canAutoHeal: true,
      });
    }
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
    openclawInstalled,
    openclawLocalMode: localMode,
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

  const declinedUntil = await currentCooldowns(wanted, now);
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
        for (const s of services) {
          declinedUntil[s] = until;
          memDeclinedUntil[s] = until; // survives a failed save_kv for this run
        }
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
        activityBus.fail(taskId, `Restart ${outcome} (details in the local log)`);
        return { status: "failed", actions: [] };
      default: {
        activityBus.fail(taskId, "Restart failed (details in the local log)");
        // Full error stays in the local log; Telegram gets a fixed category + service ids only.
        console.error(`[self-diagnosis] restart_services failed for ${services.join(", ")}:`, e);
        await telegramAlert(`Auto-heal: restart failed (${services.join(", ")})`);
        return { status: "failed", actions: [] };
      }
    }
  }

  // A per-service expired: means that service was not run (awaiting), not a failure.
  const expired = results.filter((r) => !r.ok && approvalOutcome(r.error ?? "") === "expired");
  const ran = results.filter((r) => !expired.includes(r));
  const actions: HealingAction[] = ran.map((r) => ({
    service: r.service,
    action: r.service.startsWith("docker:") ? "restart_docker" : "restart_launchagent",
    success: r.ok,
    message: r.ok ? "restarted" : (r.error ?? "restart failed"),
    timestamp: now,
  }));
  const failed = actions.filter((a) => !a.success);
  const allTriedOk = failed.length === 0 && expired.length === 0 && services.every((s) => results.some((r) => r.service === s && r.ok));
  for (const f of failed) {
    console.error(`[self-diagnosis] restart failed for ${f.service}: ${f.message}`);
    // Only allow-listed service ids from our own request reach Telegram.
    const id = services.includes(f.service) ? f.service : "unknown service";
    await telegramAlert(`Auto-heal: restart failed (${id})`);
  }
  if (allTriedOk) activityBus.complete(taskId, `${actions.length} service(s) restarted`);
  else if (failed.length === 0) activityBus.complete(taskId, `${expired.length} restart(s) expired before running`);
  else activityBus.fail(taskId, `${failed.length}/${actions.length} restart(s) failed`);

  try {
    await invoke<void>("store_session_log", {
      entry: {
        id: `heal-${Date.now()}`,
        timestamp: Date.now(),
        messageCount: 0,
        // The raw restart error stays in the console log; the saved report gets a fixed word.
        summary: `[Auto-Heal Report]\n${actions.map((a) => `${a.service}: ${a.action} → ${a.success ? "OK (restarted)" : "FAILED (see local log)"}`).join("\n")}`,
        provider: "system",
        model: "self-diagnosis",
      },
    });
  } catch {}

  return { status: allTriedOk ? "addressed" : failed.length > 0 ? "failed" : "awaiting", actions };
}
