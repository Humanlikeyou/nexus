/**
 * Terrance Self-Diagnosis + Auto-Healing
 *
 * Checks health of:
 *  - OpenClaw gateway (port 18789)
 *  - n8n (Docker container stack-n8n-1)
 *  - Ollama (port 11434)
 *
 * Auto-heal actions:
 *  - Restart OpenClaw via existing restartOpenclaw()
 *  - Restart Docker containers via Rust commands
 *  - Alert via Telegram if unable to self-heal
 */

import { invoke } from "@tauri-apps/api/core";
import type { DiagnosticReport, DiagnosticIssue, HealingAction } from "@/lib/types";
import { loadContext, saveContext } from "./continuity";
import { activityBus } from "./activityBus";

// Track heal attempts to prevent spam (max 1 per service per hour)
const healAttempts: Map<string, number> = new Map();
const HEAL_COOLDOWN_MS = 60 * 60 * 1000; // 1 hour

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

export async function autoHeal(report: DiagnosticReport): Promise<HealingAction[]> {
  const actions: HealingAction[] = [];
  const now = Date.now();
  const taskId = activityBus.register("Auto-Heal", "auto_heal", {
    detail: `${report.issues.filter((i) => i.canAutoHeal).length} healable issue(s)`,
  });

  for (const issue of report.issues) {
    if (!issue.canAutoHeal) continue;

    // Check cooldown
    const lastAttempt = healAttempts.get(issue.service) ?? 0;
    if (now - lastAttempt < HEAL_COOLDOWN_MS) {
      console.log(`[self-diagnosis] Skipping ${issue.service} heal — cooldown active`);
      continue;
    }

    healAttempts.set(issue.service, now);
    let action: HealingAction;

    switch (issue.service) {
      case "openclaw": {
        try {
          const result = await invoke<string>("restart_openclaw");
          action = {
            service: "openclaw",
            action: "restart_launchagent",
            success: true,
            message: result,
            timestamp: now,
          };
        } catch (e) {
          action = {
            service: "openclaw",
            action: "restart_launchagent",
            success: false,
            message: String(e),
            timestamp: now,
          };
        }
        break;
      }

      case "n8n": {
        try {
          const result = await invoke<string>("restart_docker_service", { name: "stack-n8n-1" });
          action = {
            service: "n8n",
            action: "restart_docker",
            success: true,
            message: result,
            timestamp: now,
          };
        } catch (e) {
          action = {
            service: "n8n",
            action: "restart_docker",
            success: false,
            message: String(e),
            timestamp: now,
          };
        }
        break;
      }

      default:
        continue;
    }

    actions.push(action);

    // If heal failed, send Telegram alert
    if (!action.success) {
      try {
        await invoke<void>("send_telegram_message", {
          text: `⚠️ *Terrance Auto-Heal Failed*\n\nService: \`${action.service}\`\nAction: ${action.action}\nError: ${action.message}\n\nManual intervention may be required.`,
        });
      } catch {
        console.error("[self-diagnosis] Failed to send Telegram alert");
      }
    }
  }

  const failed = actions.filter((a) => !a.success).length;
  if (failed > 0) {
    activityBus.fail(taskId, `${failed}/${actions.length} heal(s) failed`);
  } else {
    activityBus.complete(taskId, `${actions.length} service(s) healed`);
  }

  // Store healing actions as a session log if any actions taken
  if (actions.length > 0) {
    const summary = actions
      .map((a) => `${a.service}: ${a.action} → ${a.success ? "OK" : "FAILED"} (${a.message})`)
      .join("\n");
    try {
      await invoke<void>("store_session_log", {
        entry: {
          id: `heal-${Date.now()}`,
          timestamp: Date.now(),
          messageCount: 0,
          summary: `[Auto-Heal Report]\n${summary}`,
          provider: "system",
          model: "self-diagnosis",
        },
      });
    } catch {}
  }

  return actions;
}
