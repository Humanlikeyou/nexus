/**
 * Telegram Bridge
 *
 * Polls for inbound Telegram messages, parses commands, executes state
 * mutations, and confirms via reply. Free text goes to the LLM.
 *
 * Commands:
 *   /goal add <title>
 *   /goal list
 *   /goal done <title>
 *   /job create <name> | <schedule> | <prompt>
 *   /job list
 *   /job toggle <name|id>
 *   /job delete <name|id>
 *   /agent spawn <name> | <purpose> | <schedule>
 *   /agent list
 *   /agent stop <name|id>
 *   /status
 *   /summary
 *   /research <topic>
 *   /deliberate <topic>
 */

import { invoke } from "@tauri-apps/api/core";
import { getConfig, pollTelegramUpdates, readGoals, saveGoals, deleteGoal } from "@/lib/api";
import { buildRouterConfig } from "@/lib/llm-router";
import { loadContext, saveContext, buildSystemPrompt, generateDailySummary, generateMiddayCorrection, generateEveningIntegration, jobFailureReason } from "./continuity";
import { listCustomJobs, createCustomJob, deleteCustomJob, toggleCustomJob } from "./customJobs";
import {
  listSubagents,
  createSubagent,
  findSubagent,
  stopSubagent,
  toggleSubagent,
  deleteSubagent,
  addSubagentJob,
} from "./subagents";
import { runResearchTask } from "./research";
import { deliberate } from "./deliberation";
import { activityBus } from "./activityBus";
import { getSchedulerStatus } from "./scheduler";
import type { Goal, RouterResponse, TelegramUpdate } from "@/lib/types";

// ─── State ──────────────────────────────────────────────────────────────────

const POLL_INTERVAL_MS = 15_000; // 15 seconds
let _pollIntervalId: ReturnType<typeof setInterval> | null = null;
let _polling = false;

// ─── Lifecycle ──────────────────────────────────────────────────────────────

export function startTelegramBridge(): void {
  if (_pollIntervalId) return;
  console.log("[telegram-bridge] Started (polling every 15s).");

  // First poll after 10s delay
  setTimeout(pollOnce, 10_000);
  _pollIntervalId = setInterval(pollOnce, POLL_INTERVAL_MS);
}

export function stopTelegramBridge(): void {
  if (_pollIntervalId) {
    clearInterval(_pollIntervalId);
    _pollIntervalId = null;
  }
  console.log("[telegram-bridge] Stopped.");
}

// ─── Polling ────────────────────────────────────────────────────────────────

async function pollOnce(): Promise<void> {
  if (_polling) return;
  _polling = true;

  try {
    const ctx = await loadContext();
    const lastId = ctx.lastTelegramUpdateId ?? 0;
    const updates = await pollTelegramUpdates(lastId);

    if (updates.length === 0) {
      _polling = false;
      return;
    }

    // On first poll (lastId === 0), just establish the baseline — don't process
    if (lastId === 0) {
      const maxId = Math.max(...updates.map((u) => u.updateId));
      const freshCtx = await loadContext();
      await saveContext({ ...freshCtx, lastTelegramUpdateId: maxId });
      console.log(`[telegram-bridge] Baseline established at update_id=${maxId}`);
      _polling = false;
      return;
    }

    // Process each new message
    for (const update of updates) {
      await processMessage(update);
    }

    // Update last processed ID
    const maxId = Math.max(...updates.map((u) => u.updateId));
    const freshCtx = await loadContext();
    await saveContext({ ...freshCtx, lastTelegramUpdateId: maxId });
  } catch (e) {
    console.error("[telegram-bridge] Poll error:", e);
  } finally {
    _polling = false;
  }
}

// ─── Message Processing ─────────────────────────────────────────────────────

async function processMessage(update: TelegramUpdate): Promise<void> {
  const text = update.text.trim();
  console.log(`[telegram-bridge] Received: "${text}" from ${update.fromName}`);

  const taskId = activityBus.register("Telegram Bridge", "telegram_bridge", {
    detail: text.slice(0, 80),
  });

  try {
    // Acknowledge immediately so the user knows we heard them
    const ack = getAckMessage(text);
    if (ack) {
      await invoke<void>("send_telegram_message", { text: ack }).catch(() => {});
    }

    let reply: string;

    if (text.startsWith("/goal ")) {
      reply = await handleGoalCommand(text.slice(6).trim());
    } else if (text.startsWith("/job ")) {
      reply = await handleJobCommand(text.slice(5).trim());
    } else if (text.startsWith("/agent ")) {
      reply = await handleAgentCommand(text.slice(7).trim());
    } else if (text === "/status") {
      reply = await handleStatus();
    } else if (text === "/summary") {
      reply = await handleSummary();
    } else if (text.startsWith("/research ")) {
      reply = await handleResearch(text.slice(10).trim());
    } else if (text.startsWith("/deliberate ")) {
      reply = await handleDeliberate(text.slice(12).trim());
    } else if (text === "/midday") {
      reply = await handleMidday();
    } else if (text === "/evening") {
      reply = await handleEvening();
    } else if (text.startsWith("/")) {
      reply = `Unknown command: ${text.split(" ")[0]}\n\nAvailable: /goal, /job, /agent, /status, /summary, /research, /deliberate, /midday, /evening`;
    } else {
      // Free text → LLM
      reply = await handleFreeText(text);
    }

    // Send the actual result
    await invoke<void>("send_telegram_message", { text: reply });
    activityBus.complete(taskId, "Replied");
  } catch (e) {
    activityBus.fail(taskId, jobFailureReason(e));
    console.error("[telegram-bridge] Process error:", e);
    try {
      await invoke<void>("send_telegram_message", {
        text: `Error processing message: ${jobFailureReason(e)}`,
      });
    } catch {}
  }
}

// ─── Command Handlers ───────────────────────────────────────────────────────

async function handleGoalCommand(args: string): Promise<string> {
  if (args.startsWith("add ")) {
    const title = args.slice(4).trim();
    if (!title) return "Usage: /goal add <title>";

    const goals = await readGoals();
    const newGoal: Goal = {
      id: `goal-${Date.now()}`,
      title,
      category: "personal",
      target: 100,
      current: 0,
      unit: "%",
      deadline: null,
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
    };
    goals.push(newGoal);
    await saveGoals(goals);
    return `Goal added: "${title}"\n\nActive goals: ${goals.length}`;
  }

  if (args === "list") {
    const goals = await readGoals();
    if (goals.length === 0) return "No active goals.";
    const list = goals
      .map((g, i) => `${i + 1}. ${g.title} (${g.current}/${g.target} ${g.unit})`)
      .join("\n");
    return `Active Goals:\n${list}`;
  }

  if (args.startsWith("done ")) {
    const query = args.slice(5).trim().toLowerCase();
    if (!query) return "Usage: /goal done <title or number>";

    const goals = await readGoals();
    // Try by number first
    const num = parseInt(query, 10);
    let target: Goal | undefined;
    if (!isNaN(num) && num >= 1 && num <= goals.length) {
      target = goals[num - 1];
    } else {
      target = goals.find(
        (g) => g.title.toLowerCase() === query || g.title.toLowerCase().includes(query),
      );
    }

    if (!target) return `No goal matching "${query}"`;
    await deleteGoal(target.id);
    return `Goal completed: "${target.title}"`;
  }

  return "Usage: /goal [add <title> | list | done <title>]";
}

async function handleJobCommand(args: string): Promise<string> {
  if (args.startsWith("create ")) {
    const parts = args.slice(7).split("|").map((s) => s.trim());
    if (parts.length < 3) return "Usage: /job create <name> | <schedule> | <prompt>";

    const [name, schedule, ...promptParts] = parts;
    const prompt = promptParts.join(" | ");
    const job = await createCustomJob(name, prompt, schedule);
    if (!job) return `Failed to create job. Invalid schedule: "${schedule}"`;
    return `Job created: "${name}"\nSchedule: ${job.parsedSchedule?.description ?? schedule}`;
  }

  if (args === "list") {
    const jobs = await listCustomJobs();
    if (jobs.length === 0) return "No custom jobs.";
    const list = jobs
      .map(
        (j, i) =>
          `${i + 1}. ${j.enabled ? "▶" : "⏸"} ${j.name} — ${j.parsedSchedule?.description ?? j.schedule}${j.silent ? " [silent]" : ""}`,
      )
      .join("\n");
    return `Custom Jobs:\n${list}`;
  }

  if (args.startsWith("toggle ")) {
    const query = args.slice(7).trim().toLowerCase();
    const jobs = await listCustomJobs();
    const job = jobs.find(
      (j) => j.id === query || j.name.toLowerCase().includes(query),
    );
    if (!job) return `No job matching "${query}"`;
    await toggleCustomJob(job.id);
    return `Job "${job.name}" ${job.enabled ? "paused" : "resumed"}`;
  }

  if (args.startsWith("delete ")) {
    const query = args.slice(7).trim().toLowerCase();
    const jobs = await listCustomJobs();
    const job = jobs.find(
      (j) => j.id === query || j.name.toLowerCase().includes(query),
    );
    if (!job) return `No job matching "${query}"`;
    await deleteCustomJob(job.id);
    return `Job deleted: "${job.name}"`;
  }

  return "Usage: /job [create <name> | <schedule> | <prompt> | list | toggle <name> | delete <name>]";
}

async function handleAgentCommand(args: string): Promise<string> {
  if (args.startsWith("spawn ")) {
    const parts = args.slice(6).split("|").map((s) => s.trim());
    if (parts.length < 3) return "Usage: /agent spawn <name> | <purpose> | <schedule>";

    const [name, purpose, schedule] = parts;
    const agent = await createSubagent(name, purpose, schedule);
    if (!agent) return `Failed to create agent. Invalid schedule: "${schedule}"`;
    return `Agent spawned: "${name}"\nPurpose: ${purpose}\nSchedule: ${agent.parsedSchedule?.description ?? schedule}`;
  }

  if (args === "list") {
    const agents = await listSubagents();
    if (agents.length === 0) return "No subagents.";
    const list = agents
      .map((a, i) => {
        const statusIcon = a.status === "active" ? "▶" : a.status === "paused" ? "⏸" : "⏹";
        const jobCount = a.jobs.length > 0 ? ` (${a.jobs.length} jobs)` : "";
        return `${i + 1}. ${statusIcon} ${a.name} — ${a.purpose.slice(0, 60)}${jobCount}\n   Schedule: ${a.parsedSchedule?.description ?? a.schedule}`;
      })
      .join("\n");
    return `Subagents:\n${list}`;
  }

  if (args.startsWith("stop ")) {
    const query = args.slice(5).trim();
    const agent = await findSubagent(query);
    if (!agent) return `No agent matching "${query}"`;
    await stopSubagent(agent.id);
    return `Agent stopped: "${agent.name}"`;
  }

  if (args.startsWith("pause ") || args.startsWith("resume ")) {
    const cmd = args.startsWith("pause ") ? "pause" : "resume";
    const query = args.slice(cmd.length + 1).trim();
    const agent = await findSubagent(query);
    if (!agent) return `No agent matching "${query}"`;
    const updated = await toggleSubagent(agent.id);
    return updated
      ? `Agent "${agent.name}" ${updated.status === "active" ? "resumed" : "paused"}`
      : `Failed to toggle agent`;
  }

  if (args.startsWith("delete ")) {
    const query = args.slice(7).trim();
    const agent = await findSubagent(query);
    if (!agent) return `No agent matching "${query}"`;
    await deleteSubagent(agent.id);
    return `Agent deleted: "${agent.name}"`;
  }

  if (args.startsWith("addjob ")) {
    // /agent addjob <agentName> | <jobName> | <schedule> | <prompt>
    const parts = args.slice(7).split("|").map((s) => s.trim());
    if (parts.length < 4) return "Usage: /agent addjob <agent> | <name> | <schedule> | <prompt>";

    const [agentQuery, jobName, schedule, ...promptParts] = parts;
    const agent = await findSubagent(agentQuery);
    if (!agent) return `No agent matching "${agentQuery}"`;
    const job = await addSubagentJob(agent.id, jobName, promptParts.join(" | "), schedule);
    if (!job) return `Failed to add job. Invalid schedule: "${schedule}"`;
    return `Job added to "${agent.name}": "${jobName}" (${schedule})`;
  }

  return "Usage: /agent [spawn <name> | <purpose> | <schedule> | list | stop <name> | pause <name> | resume <name> | delete <name> | addjob <agent> | <name> | <schedule> | <prompt>]";
}

async function handleStatus(): Promise<string> {
  const status = getSchedulerStatus();
  const agents = await listSubagents();
  const jobs = await listCustomJobs();
  const activeTasks = activityBus.getActiveTasks();

  const activeAgents = agents.filter((a) => a.status === "active").length;
  const enabledJobs = jobs.filter((j) => j.enabled).length;

  const lines = [
    "System Status",
    `Scheduler: ${status.running ? "running" : "stopped"}`,
    `Active tasks: ${activeTasks.length}`,
    `Subagents: ${activeAgents}/${agents.length} active`,
    `Custom jobs: ${enabledJobs}/${jobs.length} enabled`,
    "",
    "Last runs:",
    `  Daily Summary: ${formatAgo(status.lastDailySummary)}`,
    `  Health Check: ${formatAgo(status.lastHealthCheck)}`,
    `  Consolidation: ${formatAgo(status.lastConsolidation)}`,
    `  Reasoning: ${formatAgo(status.lastReasoning)}`,
  ];

  if (activeTasks.length > 0) {
    lines.push("", "Running now:");
    for (const task of activeTasks) {
      lines.push(`  - ${task.name}: ${task.detail ?? "in progress"}`);
    }
  }

  return lines.join("\n");
}

async function handleSummary(): Promise<string> {
  const result = await generateDailySummary();
  return result.sent
    ? "Daily summary generated and sent."
    : "Failed to generate summary.";
}

async function handleResearch(topic: string): Promise<string> {
  if (!topic) return "Usage: /research <topic>";
  const task = await runResearchTask(topic);
  if (task.status === "error") {
    return `Research failed: ${task.summary ?? "Unknown error"}`;
  }
  return `[Research: ${topic}]\n\n${task.summary ?? "No results."}`;
}

async function handleDeliberate(topic: string): Promise<string> {
  if (!topic) return "Usage: /deliberate <topic>";
  const result = await deliberate(topic);
  if (!result) return "Deliberation failed.";
  return (
    `[Deliberation: ${topic}]\n\n` +
    `BULL CASE:\n${result.bullCase}\n\n` +
    `BEAR CASE:\n${result.bearCase}\n\n` +
    `SYNTHESIS:\n${result.synthesis}\n\n` +
    `Confidence: ${result.confidence}`
  );
}

async function handleMidday(): Promise<string> {
  const result = await generateMiddayCorrection();
  return result.sent
    ? result.message ?? "Midday correction sent."
    : "Failed to generate midday correction.";
}

async function handleEvening(): Promise<string> {
  const result = await generateEveningIntegration();
  return result.sent
    ? result.message ?? "Evening integration sent."
    : "Failed to generate evening integration.";
}

async function handleFreeText(text: string): Promise<string> {
  try {
    const systemPrompt = await buildSystemPrompt();
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    const fullPrompt = `${systemPrompt}\n\nUser message (via Telegram): ${text}`;

    const resp = await invoke<RouterResponse>("chat_with_router", {
      text: fullPrompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
    });

    return resp.content.trim();
  } catch (e) {
    console.error("[telegram-bridge] chat error:", e);
    return `I couldn't process that: ${jobFailureReason(e)}`;
  }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/**
 * Quick ack message so the user knows we received their input.
 * Returns null for fast commands that don't need an ack (e.g. /status, simple goal list).
 */
function getAckMessage(text: string): string | null {
  // Skip ack for commands that resolve near-instantly
  if (text === "/status" || text === "/goal list" || text === "/job list" || text === "/agent list") {
    return null;
  }
  // Skip ack for unknown commands (reply is instant)
  if (text.startsWith("/") && !text.startsWith("/goal") && !text.startsWith("/job") && !text.startsWith("/agent") && text !== "/summary" && !text.startsWith("/research") && !text.startsWith("/deliberate") && text !== "/midday" && text !== "/evening") {
    return null;
  }
  // Everything else gets an ack
  if (text === "/summary") return "Generating summary...";
  if (text.startsWith("/research")) return "Researching...";
  if (text.startsWith("/deliberate")) return "Running deliberation...";
  if (text === "/midday") return "Checking in...";
  if (text === "/evening") return "Reflecting...";
  if (text.startsWith("/agent spawn")) return "Spawning agent...";
  if (text.startsWith("/")) return "On it.";
  // Free text → LLM (slowest path)
  return "Heard you. Thinking...";
}

function formatAgo(ts: number | null): string {
  if (!ts) return "never";
  const diffMs = Date.now() - ts;
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  return `${Math.floor(diffHr / 24)}d ago`;
}
