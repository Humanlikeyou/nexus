/**
 * Custom Cron Jobs
 *
 * User-defined scheduled tasks with natural-language scheduling,
 * LLM-powered execution, Telegram delivery, and [SILENT] suppression
 * when there's nothing meaningful to report.
 *
 * Jobs are persisted to disk and run by the main scheduler tick.
 */

import { invokeBound } from "../../../bridge";
import { getConfig } from "../../lib/api";
import { buildRouterConfig } from "../../lib/llm-router";
import { parseSchedule, shouldRunNow, type ParsedSchedule } from "./cronParser";
import type { RouterResponse } from "../../lib/types";
import { activityBus } from "./activityBus";
import { jobFailureReason } from "./continuity";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface CustomJob {
  id: string;
  name: string;
  prompt: string;
  schedule: string;
  parsedSchedule: ParsedSchedule | null;
  enabled: boolean;
  silent: boolean; // If true, suppress Telegram delivery when result is trivial
  lastRun: number;
  lastResult: string | null;
  createdAt: number;
}

export interface JobRunResult {
  jobId: string;
  output: string;
  delivered: boolean;
  silent: boolean;
  timestamp: number;
}

// ─── Storage ────────────────────────────────────────────────────────────────

const JOBS_FILE = "custom_cron_jobs.json";

async function loadJobsFromDisk(): Promise<CustomJob[]> {
  try {
    const raw = await invokeBound<string>("load_custom_jobs");
    return JSON.parse(raw);
  } catch {
    return [];
  }
}

async function saveJobsToDisk(jobs: CustomJob[]): Promise<void> {
  try {
    await invokeBound<void>("save_custom_jobs", { data: JSON.stringify(jobs, null, 2) });
  } catch (e) {
    console.error("[custom-jobs] Failed to save:", e);
  }
}

// ─── In-memory cache ────────────────────────────────────────────────────────

let _jobs: CustomJob[] | null = null;

async function getJobs(): Promise<CustomJob[]> {
  if (!_jobs) {
    _jobs = await loadJobsFromDisk();
  }
  return _jobs;
}

// ─── CRUD ───────────────────────────────────────────────────────────────────

export async function listCustomJobs(): Promise<CustomJob[]> {
  return getJobs();
}

export async function createCustomJob(
  name: string,
  prompt: string,
  schedule: string,
  silent = true,
): Promise<CustomJob | null> {
  const parsed = parseSchedule(schedule);
  if (!parsed) {
    console.error(`[custom-jobs] Invalid schedule: "${schedule}"`);
    return null;
  }

  const job: CustomJob = {
    id: `job-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
    name,
    prompt,
    schedule,
    parsedSchedule: parsed,
    enabled: true,
    silent,
    lastRun: 0,
    lastResult: null,
    createdAt: Date.now(),
  };

  const jobs = await getJobs();
  jobs.push(job);
  _jobs = jobs;
  await saveJobsToDisk(jobs);

  console.log(`[custom-jobs] Created: "${name}" (${parsed.description})`);
  return job;
}

export async function deleteCustomJob(jobId: string): Promise<boolean> {
  const jobs = await getJobs();
  const idx = jobs.findIndex((j) => j.id === jobId);
  if (idx === -1) return false;
  jobs.splice(idx, 1);
  _jobs = jobs;
  await saveJobsToDisk(jobs);
  return true;
}

export async function toggleCustomJob(jobId: string): Promise<boolean> {
  const jobs = await getJobs();
  const job = jobs.find((j) => j.id === jobId);
  if (!job) return false;
  job.enabled = !job.enabled;
  _jobs = jobs;
  await saveJobsToDisk(jobs);
  return true;
}

// ─── Execution ──────────────────────────────────────────────────────────────

/** Trivial output patterns — suppress delivery if output matches */
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
 * Run a single custom job: send prompt to LLM, optionally deliver via Telegram.
 */
export async function runJob(job: CustomJob): Promise<JobRunResult> {
  const now = Date.now();
  const taskId = activityBus.register(`Job: ${job.name}`, "custom_job", {
    detail: job.prompt.slice(0, 80),
  });

  try {
    const cfg = await getConfig();
    const routerConfig = buildRouterConfig(cfg);
    const apiKey = cfg.anthropic_api_key ?? undefined;

    activityBus.update(taskId, "running", "Calling LLM...");
    const resp = await invokeBound<RouterResponse>("chat_with_router", {
      text: job.prompt,
      config: routerConfig,
      tags: [],
      forceProvider: undefined,
      anthropicApiKey: apiKey,
    });

    const output = resp.content.trim();
    const shouldSuppress = job.silent && isTrivialOutput(output);

    // Deliver via Telegram unless suppressed
    let delivered = false;
    let deliveryError: string | null = null;
    if (!shouldSuppress) {
      try {
        activityBus.update(taskId, "running", "Sending to Telegram...");
        await invokeBound<void>("send_telegram_message", {
          text: `[${job.name}]\n\n${output}`,
        });
        delivered = true;
      } catch (e) {
        console.error(`[custom-jobs] Telegram delivery failed for "${job.name}":`, e);
        deliveryError = jobFailureReason(e, "delivery failed");
      }
    } else {
      console.log(`[custom-jobs] Suppressed silent output for "${job.name}"`);
    }

    // Activity never shows raw errors (they may carry URLs or provider text); a failed
    // delivery is a failure, not "LLM complete".
    if (deliveryError) activityBus.fail(taskId, `Not delivered: ${deliveryError}`);
    else activityBus.complete(taskId, shouldSuppress ? "Silent (suppressed)" : "Delivered");
    return { jobId: job.id, output, delivered, silent: shouldSuppress, timestamp: now };
  } catch (e) {
    const reason = jobFailureReason(e);
    activityBus.fail(taskId, reason);
    console.error(`[custom-jobs] Run failed for "${job.name}":`, e);
    return { jobId: job.id, output: `Error: ${reason}`, delivered: false, silent: false, timestamp: now };
  }
}

/**
 * Tick handler — called from the main scheduler every 5 minutes.
 * Checks all custom jobs and runs those that are due.
 */
export async function tickCustomJobs(): Promise<JobRunResult[]> {
  const jobs = await getJobs();
  const results: JobRunResult[] = [];

  for (const job of jobs) {
    if (!job.enabled || !job.parsedSchedule) continue;
    if (!shouldRunNow(job.parsedSchedule, job.lastRun)) continue;

    console.log(`[custom-jobs] Running: "${job.name}"`);
    const result = await runJob(job);

    // Update job state
    job.lastRun = result.timestamp;
    job.lastResult = result.output;
    results.push(result);
  }

  if (results.length > 0) {
    _jobs = jobs;
    await saveJobsToDisk(jobs);
  }

  return results;
}

/** Invalidate cache — call after external changes */
export function invalidateJobsCache(): void {
  _jobs = null;
}
