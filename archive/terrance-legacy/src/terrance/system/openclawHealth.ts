/**
 * openclawHealth.ts — OpenClaw runtime health module
 *
 * Wraps the `get_openclaw_health` Tauri command and exposes:
 *   - Individual check functions for granular use
 *   - getOpenClawHealth()  — full JSON health snapshot
 *   - generateSystemReport() — human-readable text report
 *   - pushHealthToN8n()    — POST health data to Amara n8n ingress
 *
 * All heavy lifting (port check, process probe, config parsing) is done
 * in Rust via the Tauri command; this module is purely orchestration.
 */

import { invoke } from "@tauri-apps/api/core";
import type { OpenClawHealth } from "@/lib/types";

// Re-export the type for convenience
export type { OpenClawHealth };

// ─── Core command wrapper ─────────────────────────────────────────────────────

/** Full health snapshot — the primary entry point. */
export function getOpenClawHealth(): Promise<OpenClawHealth> {
  return invoke<OpenClawHealth>("get_openclaw_health");
}

// ─── Individual check functions ───────────────────────────────────────────────
// These each call the Tauri command and return the relevant field.
// For bulk checks, prefer getOpenClawHealth() and destructure the result.

/** Returns true if the OpenClaw gateway process is alive (TCP port reachable). */
export async function checkGatewayProcess(): Promise<boolean> {
  return (await getOpenClawHealth()).gateway;
}

/** Returns true if TCP port 18789 accepts connections. */
export async function checkPort(): Promise<boolean> {
  return (await getOpenClawHealth()).gateway;
}

/** Returns true if the gateway HTTP layer responds (RPC probe passed). */
export async function checkRPC(): Promise<boolean> {
  return (await getOpenClawHealth()).rpc;
}

/** Returns the installed openclaw version string, or null if not reachable. */
export async function getVersion(): Promise<string | null> {
  return (await getOpenClawHealth()).version;
}

/**
 * Returns "configured" if a memory search provider is ready,
 * "missing" if provider is "local" with no model file installed.
 */
export async function checkMemorySearch(): Promise<"configured" | "missing"> {
  return (await getOpenClawHealth()).memory_search;
}

/** Returns channel connectivity status (telegram, whatsapp). */
export async function getChannelStatus(): Promise<OpenClawHealth["channels"]> {
  return (await getOpenClawHealth()).channels;
}

// ─── Human-readable report ────────────────────────────────────────────────────

/**
 * Generates a plain-text daily system status report.
 *
 * Example output:
 *   SYSTEM STATUS — AMARA
 *   OpenClaw: HEALTHY
 *   Version: 2026.2.22-2
 *   Gateway: reachable
 *   Memory Search: configured
 *   Telegram: connected
 */
export async function generateSystemReport(): Promise<string> {
  const h = await getOpenClawHealth();

  const statusLine = h.status.toUpperCase();
  const gatewayLine = h.gateway && h.rpc ? "reachable" : "unreachable";
  const ts = new Date(h.timestamp).toLocaleString();

  return [
    "SYSTEM STATUS — AMARA",
    "─".repeat(32),
    `OpenClaw:      ${statusLine}`,
    `Version:       ${h.version ?? "unknown"}`,
    `Gateway:       ${gatewayLine}`,
    `Memory Search: ${h.memory_search}`,
    `Telegram:      ${h.channels.telegram}`,
    `WhatsApp:      ${h.channels.whatsapp}`,
    "─".repeat(32),
    `Checked:       ${ts}`,
  ].join("\n");
}

// ─── n8n integration ─────────────────────────────────────────────────────────

const N8N_HEALTH_MONITOR = "http://localhost:5678/webhook/terrance/openclaw/health";

/**
 * Pushes a health snapshot to the Amara n8n ingress gateway
 * as an `openclaw.health` event.  n8n routes it through the
 * OpenClaw Health Monitor workflow for alerting and logging.
 *
 * Pass `secret` (from NexusConfig.n8n_webhook_secret) to include
 * an `X-Nexus-Secret` header that your n8n Code node should validate.
 *
 * Safe to call on every UI refresh cycle — fire-and-forget.
 */
export async function pushHealthToN8n(
  health: OpenClawHealth,
  secret?: string,
): Promise<void> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (secret) {
    headers["X-Nexus-Secret"] = secret;
  }
  await fetch(N8N_HEALTH_MONITOR, {
    method: "POST",
    headers,
    body: JSON.stringify({
      trace_id: `health-${Date.now()}`,
      ...health,
    }),
  });
}
