/**
 * Subagent System
 *
 * Autonomous agents with their own purpose, schedule, and accumulated memory.
 * Spawnable via Telegram commands, visible in ActivityMonitor.
 *
 * Each subagent:
 *  - Has a focused purpose (e.g., "Monitor crypto news")
 *  - Runs on its own cron schedule
 *  - Accumulates memory across runs (rolling window of 10)
 *  - Can have child cron jobs
 *  - Reports activity via activityBus
 *  - Delivers results via Telegram (unless trivial)
 */

import { invoke } from "@tauri-apps/api/core";
import { getConfig } from "@/lib/api";
import { buildRouterConfig } from "@/lib/llm-router";
import { parseSchedule, shouldRunNow, type ParsedSchedule } from "./cronParser";
import { jobFailureReason, loadIdentity } from "./continuity";
import { activityBus } from "./activityBus";
import type { RouterResponse } from "@/lib/types";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface SubagentJob {
  id: string;
  name: string;
  prompt: string;
  schedule: string;
  parsedSchedule: ParsedSchedule | null;
  enabled: boolean;
  lastRun: number;
}

export interface Subagent {
  id: string;
  name: string;
  purpose: string;
  schedule: string;
  parsedSchedule: ParsedSchedule | null;
  status: "active" | "paused" | "stopped";
  lastRun: number;
  lastOutput: string | null;
  memory: string[];
  createdAt: number;
  jobs: SubagentJob[];
}

// ─── Storage ────────────────────────────────────────────────────────────────

let _cache: Subagent[] | null = null;

async function loadFromDisk(): Promise<Subagent[]> {
  try {
    const raw = await invoke<string>("load_subagents");
    return JSON.parse(raw);
  } catch {
    return [];
  }
}

async function saveToDisk(agents: Subagent[]): Promise<void> {
  try {
    await invoke<void>("save_subagents", { data: JSON.stringify(agents, null, 2) });
  } catch (e) {
    console.error("[subagents] Failed to save:", e);
  }
}

async function getAgents(): Promise<Subagent[]> {
  if (!_cache) {
    _cache = await loadFromDisk();
  }
  return _cache;
}

// ─── CRUD ───────────────────────────────────────────────────────────────────

export async function listSubagents(): Promise<Subagent[]> {
  return getAgents();
}

export async function createSubagent(
  name: string,
  purpose: string,
  schedule: string,
): Promise<Subagent | null> {
  const parsed = parseSchedule(schedule);
  if (!parsed) {
    console.error(`[subagents] Invalid schedule: "${schedule}"`);
    return null;
  }

  const agent: Subagent = {
    id: `agent-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
    name,
    purpose,
    schedule,
    parsedSchedule: parsed,
    status: "active",
    lastRun: 0,
    lastOutput: null,
    memory: [],
    createdAt: Date.now(),
    jobs: [],
  };

  const agents = await getAgents();
  agents.push(agent);
  _cache = agents;
  await saveToDisk(agents);

  console.log(`[subagents] Created: "${name}" — ${purpose} (${parsed.description})`);
  return agent;
}

export async function deleteSubagent(agentId: string): Promise<boolean> {
  const agents = await getAgents();
  const idx = agents.findIndex((a) => a.id === agentId);
  if (idx === -1) return false;
  agents.splice(idx, 1);
  _cache = agents;
  await saveToDisk(agents);
  return true;
}

export async function toggleSubagent(agentId: string): Promise<Subagent | null> {
  const agents = await getAgents();
  const agent = agents.find((a) => a.id === agentId);
  if (!agent) return null;
  agent.status = agent.status === "active" ? "paused" : "active";
  _cache = agents;
  await saveToDisk(agents);
  return agent;
}

export async function stopSubagent(agentId: string): Promise<boolean> {
  const agents = await getAgents();
  const agent = agents.find((a) => a.id === agentId);
  if (!agent) return false;
  agent.status = "stopped";
  _cache = agents;
  await saveToDisk(agents);
  return true;
}

/** Find a subagent by name (case-insensitive) or ID */
export async function findSubagent(nameOrId: string): Promise<Subagent | null> {
  const agents = await getAgents();
  const lower = nameOrId.toLowerCase();
  return (
    agents.find((a) => a.id === nameOrId) ??
    agents.find((a) => a.name.toLowerCase() === lower) ??
    agents.find((a) => a.name.toLowerCase().includes(lower)) ??
    null
  );
}

// ─── Child Job CRUD ─────────────────────────────────────────────────────────

export async function addSubagentJob(
  agentId: string,
  name: string,
  prompt: string,
  schedule: string,
): Promise<SubagentJob | null> {
  const agents = await getAgents();
  const agent = agents.find((a) => a.id === agentId);
  if (!agent) return null;

  const parsed = parseSchedule(schedule);
  if (!parsed) return null;

  const job: SubagentJob = {
    id: `subjob-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
    name,
    prompt,
    schedule,
    parsedSchedule: parsed,
    enabled: true,
    lastRun: 0,
  };

  agent.jobs.push(job);
  _cache = agents;
  await saveToDisk(agents);
  return job;
}

export async function deleteSubagentJob(agentId: string, jobId: string): Promise<boolean> {
  const agents = await getAgents();
  const agent = agents.find((a) => a.id === agentId);
  if (!agent) return false;
  const idx = agent.jobs.findIndex((j) => j.id === jobId);
  if (idx === -1) return false;
  agent.jobs.splice(idx, 1);
  _cache = agents;
  await saveToDisk(agents);
  return true;
}

// ─── Execution ──────────────────────────────────────────────────────────────

/** Trivial output patterns — suppress Telegram delivery */
const SILENT_PATTERNS = [
  /no\s+(?:new|notable|significant)\s+(?:updates?|changes?|activity|items?)/i,
  /nothing\s+(?:new|notable|to\s+report)/i,
  /all\s+(?:clear|good|quiet|stable)/i,
  /no\s+(?:action|intervention)\s+(?:needed|required)/i,
  /\[SILENT\]/i,
];

function isTrivialOutput(text: string): boolean {
  const trimmed = text.trim();
  if (trimmed.length < 20) return true;
  return SILENT_PATTERNS.some((p) => p.test(trimmed));
}

/**
 * Run a single subagent: build focused prompt, call LLM, store memory, deliver.
 */
async function runSubagent(agent: Subagent): Promise<void> {
  const taskId = activityBus.register(`Agent: ${agent.name}`, "subagent", {
    detail: agent.purpose.slice(0, 80),
  });

  try {
    const identity = await loadIdentity();
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    // Build focused system prompt
    const memoryBlock = agent.memory.length > 0
      ? `\nPrevious observations:\n${agent.memory.map((m, i) => `${i + 1}. ${m}`).join("\n")}`
      : "";

    const prompt =
      `${identity}\n\n` +
      `## Subagent: ${agent.name}\n` +
      `Purpose: ${agent.purpose}\n` +
      `Schedule: ${agent.parsedSchedule?.description ?? agent.schedule}\n` +
      `${memoryBlock}\n\n` +
      `Execute your purpose. Be concise and actionable. ` +
      `If there is nothing meaningful to report, respond with [SILENT].`;

    activityBus.update(taskId, "running", "Calling LLM...");
    const resp = await invoke<RouterResponse>("chat_with_router", {
      text: prompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
    });

    const output = resp.content.trim();
    agent.lastOutput = output;
    agent.lastRun = Date.now();

    // Update rolling memory (keep last 10)
    const memoryEntry = `[${new Date().toLocaleString()}] ${output.slice(0, 200)}`;
    agent.memory.push(memoryEntry);
    if (agent.memory.length > 10) {
      agent.memory = agent.memory.slice(-10);
    }

    // Deliver via Telegram unless trivial
    const trivial = isTrivialOutput(output);
    if (!trivial) {
      try {
        activityBus.update(taskId, "running", "Sending to Telegram...");
        await invoke<void>("send_telegram_message", {
          text: `[${agent.name}]\n\n${output}`,
        });
        activityBus.complete(taskId, "Delivered");
      } catch (e) {
        console.error(`[subagents] Telegram delivery failed for "${agent.name}":`, e);
        activityBus.complete(taskId, "LLM complete (Telegram failed)");
      }
    } else {
      activityBus.complete(taskId, "Silent (suppressed)");
    }
  } catch (e) {
    activityBus.fail(taskId, jobFailureReason(e));
    console.error(`[subagents] Run failed for "${agent.name}":`, e);
  }
}

/**
 * Run a single subagent child job within the parent's context.
 */
async function runSubagentJob(agent: Subagent, job: SubagentJob): Promise<void> {
  const taskId = activityBus.register(`${agent.name}: ${job.name}`, "subagent", {
    detail: job.prompt.slice(0, 80),
  });

  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const contextBlock = agent.memory.length > 0
      ? `\nAgent memory:\n${agent.memory.slice(-5).join("\n")}\n\n`
      : "";

    const prompt =
      `You are a subagent named "${agent.name}" (purpose: ${agent.purpose}).\n` +
      `${contextBlock}` +
      `Task: ${job.prompt}\n\n` +
      `Be concise. If nothing meaningful, respond with [SILENT].`;

    activityBus.update(taskId, "running", "Calling LLM...");
    const resp = await invoke<RouterResponse>("chat_with_router", {
      text: prompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
    });

    const output = resp.content.trim();
    job.lastRun = Date.now();

    const trivial = isTrivialOutput(output);
    if (!trivial) {
      try {
        await invoke<void>("send_telegram_message", {
          text: `[${agent.name} → ${job.name}]\n\n${output}`,
        });
        activityBus.complete(taskId, "Delivered");
      } catch {
        activityBus.complete(taskId, "LLM complete");
      }
    } else {
      activityBus.complete(taskId, "Silent (suppressed)");
    }
  } catch (e) {
    activityBus.fail(taskId, jobFailureReason(e));
  }
}

// ─── Tick ───────────────────────────────────────────────────────────────────

/**
 * Tick handler — called from the main scheduler every 5 minutes.
 * Checks all active subagents and their child jobs.
 */
export async function tickSubagents(): Promise<void> {
  const agents = await getAgents();
  let dirty = false;

  for (const agent of agents) {
    if (agent.status !== "active" || !agent.parsedSchedule) continue;

    // Check main agent schedule
    if (shouldRunNow(agent.parsedSchedule, agent.lastRun)) {
      console.log(`[subagents] Running: "${agent.name}"`);
      await runSubagent(agent);
      dirty = true;
    }

    // Check child jobs
    for (const job of agent.jobs) {
      if (!job.enabled || !job.parsedSchedule) continue;
      if (shouldRunNow(job.parsedSchedule, job.lastRun)) {
        console.log(`[subagents] Running job: "${agent.name}" → "${job.name}"`);
        await runSubagentJob(agent, job);
        dirty = true;
      }
    }
  }

  if (dirty) {
    _cache = agents;
    await saveToDisk(agents);
  }
}

/** Invalidate cache — call after external changes */
export function invalidateSubagentsCache(): void {
  _cache = null;
}
