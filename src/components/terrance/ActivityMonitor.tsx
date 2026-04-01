/**
 * ActivityMonitor — Live task dashboard + job manager
 *
 * Replaces CronSchedule. Shows:
 *  - LIVE TASKS: Currently running operations with elapsed time + STOP
 *  - CRON JOBS: Scheduled jobs with add/edit/delete/toggle
 *  - TIMELINE: Recent completed activity (last 24h)
 */

import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  Play,
  Pause,
  Square,
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Clock,
  CheckCircle2,
  XCircle,
  Loader2,
  AlertTriangle,
  X,
  Save,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { listActivityEntries } from "@/lib/api";
import type { ActivityEntry, CronJob } from "@/lib/types";
import type { ActivityEvent, ActivityStep } from "@/terrance/system/activityBus";
import { activityBus } from "@/terrance/system/activityBus";
import { getSchedulerStatus } from "@/terrance/system/scheduler";
import type { SchedulerStatus } from "@/terrance/system/scheduler";
import {
  listCustomJobs,
  createCustomJob,
  deleteCustomJob,
  toggleCustomJob,
  type CustomJob,
} from "@/terrance/system/customJobs";
import {
  listSubagents,
  toggleSubagent,
  deleteSubagent,
  type Subagent,
} from "@/terrance/system/subagents";
import { parseSchedule } from "@/terrance/system/cronParser";

// ─── Helpers ────────────────────────────────────────────────────────────────

function formatElapsed(startedAt: number): string {
  const diffMs = Date.now() - startedAt;
  const secs = Math.floor(diffMs / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ${secs % 60}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}

function formatDuration(ms: number): string {
  const secs = Math.floor(ms / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ${secs % 60}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}

function formatTimeAgo(ts: number): string {
  const diff = Date.now() - ts;
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

function formatNextRun(lastRun: number | null, intervalMs: number): string {
  if (!lastRun) return "pending";
  const next = lastRun + intervalMs;
  const diffMs = next - Date.now();
  if (diffMs <= 0) return "due now";
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) return `in ${diffMin}m`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `in ${diffHr}h ${diffMin % 60}m`;
  return `in ${Math.floor(diffHr / 24)}d`;
}

function statusIcon(status: string, size = 10) {
  switch (status) {
    case "running":
      return <Loader2 size={size} className="text-accent animate-spin" />;
    case "complete":
      return <CheckCircle2 size={size} className="text-success" />;
    case "error":
      return <XCircle size={size} className="text-destructive" />;
    case "cancelled":
      return <X size={size} className="text-warning" />;
    default:
      return <Clock size={size} className="text-muted-foreground/50" />;
  }
}

// ─── Section: Live Tasks ────────────────────────────────────────────────────

function LiveTasks({ tasks }: { tasks: ActivityEvent[] }) {
  // Force re-render every second to update elapsed times
  const [, setTick] = useState(0);
  useEffect(() => {
    if (tasks.length === 0) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [tasks.length]);

  if (tasks.length === 0) {
    return (
      <div className="px-2.5 py-3 text-center">
        <p className="text-[10px] font-mono text-muted-foreground/40 uppercase tracking-wider">
          No active tasks
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-1">
      {tasks.map((task) => (
        <div
          key={task.id}
          className="flex items-center justify-between px-2.5 py-2 rounded-sm border border-accent/20 bg-accent/5"
        >
          <div className="flex items-center gap-2.5 flex-1 min-w-0">
            <Loader2 size={11} className="text-accent animate-spin shrink-0" />
            <div className="min-w-0 flex-1">
              <p className="text-[11px] font-mono text-foreground/85 truncate">
                {task.name}
              </p>
              {task.detail && (
                <p className="text-[10px] font-mono text-muted-foreground/50 truncate">
                  {task.detail}
                </p>
              )}
              {/* Sub-steps */}
              {task.steps && task.steps.length > 0 && (
                <div className="flex gap-2 mt-1">
                  {task.steps.map((step, i) => (
                    <div key={i} className="flex items-center gap-1">
                      {statusIcon(step.status, 8)}
                      <span className="text-[9px] font-mono text-muted-foreground/50">
                        {step.label}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0 ml-2">
            {task.progress !== undefined && (
              <div className="w-12 h-1 bg-muted/20 rounded-full overflow-hidden">
                <div
                  className="h-full bg-accent rounded-full transition-all"
                  style={{ width: `${task.progress}%` }}
                />
              </div>
            )}
            <span className="text-[10px] font-mono text-accent tabular-nums">
              {formatElapsed(task.startedAt)}
            </span>
            <button
              onClick={() => activityBus.cancel(task.id)}
              className="p-0.5 rounded hover:bg-destructive/15 text-muted-foreground/40 hover:text-destructive transition-colors"
              title="Stop task"
            >
              <Square size={9} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

// ─── Section: Cron Jobs ─────────────────────────────────────────────────────

interface SchedulerJob {
  name: string;
  interval: string;
  lastRun: number | null;
  intervalMs: number;
  enabled: boolean;
}

function buildSchedulerJobs(status: SchedulerStatus): SchedulerJob[] {
  return [
    { name: "Daily Summary", interval: `daily @ ${status.dailySummaryHour}:00`, lastRun: status.lastDailySummary, intervalMs: 24 * 60 * 60 * 1000, enabled: true },
    { name: "Health Check", interval: "every 30m", lastRun: status.lastHealthCheck, intervalMs: 30 * 60 * 1000, enabled: true },
    { name: "Consolidation", interval: "every 6h", lastRun: status.lastConsolidation, intervalMs: 6 * 60 * 60 * 1000, enabled: true },
    { name: "Reasoning Loop", interval: "every 12h", lastRun: status.lastReasoning, intervalMs: 12 * 60 * 60 * 1000, enabled: true },
    { name: "Memory Pruning", interval: "weekly", lastRun: status.lastPrune, intervalMs: 7 * 24 * 60 * 60 * 1000, enabled: true },
  ];
}

function CronJobsSection() {
  const [status, setStatus] = useState<SchedulerStatus | null>(null);
  const [customJobs, setCustomJobs] = useState<CustomJob[]>([]);
  const [showAddForm, setShowAddForm] = useState(false);
  const [expanded, setExpanded] = useState(true);

  // Add form state
  const [newName, setNewName] = useState("");
  const [newPrompt, setNewPrompt] = useState("");
  const [newSchedule, setNewSchedule] = useState("");
  const [newSilent, setNewSilent] = useState(true);
  const [scheduleError, setScheduleError] = useState<string | null>(null);
  const [scheduleDesc, setScheduleDesc] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setStatus(getSchedulerStatus());
    try {
      const jobs = await listCustomJobs();
      setCustomJobs(jobs);
    } catch {}
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 30_000);
    return () => clearInterval(id);
  }, [refresh]);

  // Validate schedule input
  useEffect(() => {
    if (!newSchedule.trim()) {
      setScheduleError(null);
      setScheduleDesc(null);
      return;
    }
    const parsed = parseSchedule(newSchedule);
    if (parsed) {
      setScheduleError(null);
      setScheduleDesc(parsed.description);
    } else {
      setScheduleError("Invalid schedule format");
      setScheduleDesc(null);
    }
  }, [newSchedule]);

  const handleAddJob = async () => {
    if (!newName.trim() || !newPrompt.trim() || !newSchedule.trim() || scheduleError) return;
    const job = await createCustomJob(newName.trim(), newPrompt.trim(), newSchedule.trim(), newSilent);
    if (job) {
      setShowAddForm(false);
      setNewName("");
      setNewPrompt("");
      setNewSchedule("");
      setNewSilent(true);
      refresh();
    }
  };

  const handleDeleteJob = async (jobId: string) => {
    await deleteCustomJob(jobId);
    refresh();
  };

  const handleToggleJob = async (jobId: string) => {
    await toggleCustomJob(jobId);
    refresh();
  };

  const schedulerJobs = status ? buildSchedulerJobs(status) : [];

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <button
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-1 text-[10px] font-mono uppercase tracking-wider text-muted-foreground/60 hover:text-muted-foreground/80 transition-colors"
        >
          {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
          Cron Jobs
        </button>
        <button
          onClick={() => setShowAddForm(!showAddForm)}
          className="p-0.5 rounded hover:bg-accent/15 text-muted-foreground/40 hover:text-accent transition-colors"
          title="Add custom job"
        >
          <Plus size={11} />
        </button>
      </div>

      {/* Add Job Form */}
      {showAddForm && (
        <div className="p-2.5 rounded-sm border border-accent/20 bg-accent/5 space-y-2">
          <input
            type="text"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="Job name"
            className="w-full px-2 py-1.5 rounded-sm bg-[#0C1018] border border-border/35 text-[11px] font-mono text-foreground/85 placeholder:text-muted-foreground/30 focus:outline-none focus:border-accent/40"
          />
          <textarea
            value={newPrompt}
            onChange={(e) => setNewPrompt(e.target.value)}
            placeholder="Prompt for the LLM..."
            rows={2}
            className="w-full px-2 py-1.5 rounded-sm bg-[#0C1018] border border-border/35 text-[11px] font-mono text-foreground/85 placeholder:text-muted-foreground/30 focus:outline-none focus:border-accent/40 resize-none"
          />
          <div className="flex gap-2">
            <div className="flex-1">
              <input
                type="text"
                value={newSchedule}
                onChange={(e) => setNewSchedule(e.target.value)}
                placeholder='Schedule (e.g. "every 2h", "daily at 9am")'
                className={cn(
                  "w-full px-2 py-1.5 rounded-sm bg-[#0C1018] border text-[11px] font-mono text-foreground/85 placeholder:text-muted-foreground/30 focus:outline-none",
                  scheduleError ? "border-destructive/40" : "border-border/35 focus:border-accent/40",
                )}
              />
              {scheduleDesc && (
                <p className="text-[9px] font-mono text-accent/60 mt-0.5 px-1">{scheduleDesc}</p>
              )}
              {scheduleError && (
                <p className="text-[9px] font-mono text-destructive/60 mt-0.5 px-1">{scheduleError}</p>
              )}
            </div>
          </div>
          <div className="flex items-center justify-between">
            <label className="flex items-center gap-1.5 text-[10px] font-mono text-muted-foreground/50">
              <input
                type="checkbox"
                checked={newSilent}
                onChange={(e) => setNewSilent(e.target.checked)}
                className="rounded-sm"
              />
              Silent mode
            </label>
            <div className="flex gap-1.5">
              <button
                onClick={() => setShowAddForm(false)}
                className="px-2 py-1 rounded-sm text-[10px] font-mono text-muted-foreground/50 hover:text-muted-foreground/80 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleAddJob}
                disabled={!newName.trim() || !newPrompt.trim() || !newSchedule.trim() || !!scheduleError}
                className="flex items-center gap-1 px-2.5 py-1 rounded-sm bg-accent/15 text-accent text-[10px] font-mono hover:bg-accent/25 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              >
                <Save size={9} />
                Save
              </button>
            </div>
          </div>
        </div>
      )}

      {expanded && (
        <div className="space-y-0.5">
          {/* Built-in scheduler jobs */}
          {schedulerJobs.map((job, i) => (
            <div
              key={`sched-${i}`}
              className="flex items-center justify-between px-2.5 py-1.5 rounded-sm border border-border/18 bg-muted/5"
            >
              <div className="flex items-center gap-2">
                <Play size={8} className="text-success shrink-0" />
                <div>
                  <p className="text-[11px] font-mono text-foreground/85">{job.name}</p>
                  <p className="text-[9px] font-mono text-muted-foreground/40">{job.interval}</p>
                </div>
              </div>
              <div className="text-right">
                <p className="text-[10px] font-mono text-muted-foreground/50">
                  {formatNextRun(job.lastRun, job.intervalMs)}
                </p>
              </div>
            </div>
          ))}

          {/* Custom user jobs */}
          {customJobs.map((job) => (
            <div
              key={job.id}
              className="flex items-center justify-between px-2.5 py-1.5 rounded-sm border border-border/18 bg-muted/5"
            >
              <div className="flex items-center gap-2 flex-1 min-w-0">
                <button
                  onClick={() => handleToggleJob(job.id)}
                  className="shrink-0"
                  title={job.enabled ? "Disable" : "Enable"}
                >
                  {job.enabled ? (
                    <Play size={8} className="text-success" />
                  ) : (
                    <Pause size={8} className="text-muted-foreground/40" />
                  )}
                </button>
                <div className="min-w-0 flex-1">
                  <p className={cn(
                    "text-[11px] font-mono truncate",
                    job.enabled ? "text-foreground/85" : "text-muted-foreground/40",
                  )}>
                    {job.name}
                  </p>
                  <p className="text-[9px] font-mono text-muted-foreground/40 truncate">
                    {job.parsedSchedule?.description ?? job.schedule}
                    {job.silent && " [silent]"}
                  </p>
                </div>
              </div>
              <button
                onClick={() => handleDeleteJob(job.id)}
                className="p-0.5 rounded hover:bg-destructive/15 text-muted-foreground/30 hover:text-destructive transition-colors shrink-0 ml-1"
                title="Delete job"
              >
                <Trash2 size={9} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Section: Subagents ─────────────────────────────────────────────────────

function SubagentsSection() {
  const [agents, setAgents] = useState<Subagent[]>([]);
  const [expanded, setExpanded] = useState(true);
  const [expandedAgent, setExpandedAgent] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await listSubagents();
      setAgents(list);
    } catch {}
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 30_000);
    return () => clearInterval(id);
  }, [refresh]);

  if (agents.length === 0) return null;

  const handleToggle = async (agentId: string) => {
    await toggleSubagent(agentId);
    refresh();
  };

  const handleDelete = async (agentId: string) => {
    await deleteSubagent(agentId);
    refresh();
  };

  return (
    <div className="space-y-1.5">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 text-[10px] font-mono uppercase tracking-wider text-muted-foreground/60 hover:text-muted-foreground/80 transition-colors"
      >
        {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        Subagents ({agents.length})
      </button>

      {expanded && (
        <div className="space-y-0.5">
          {agents.map((agent) => (
            <div key={agent.id}>
              <div className="flex items-center justify-between px-2.5 py-1.5 rounded-sm border border-border/18 bg-muted/5">
                <div className="flex items-center gap-2 flex-1 min-w-0">
                  <button
                    onClick={() => handleToggle(agent.id)}
                    className="shrink-0"
                    title={agent.status === "active" ? "Pause" : "Resume"}
                  >
                    {agent.status === "active" ? (
                      <Play size={8} className="text-accent" />
                    ) : agent.status === "paused" ? (
                      <Pause size={8} className="text-warning" />
                    ) : (
                      <Square size={8} className="text-muted-foreground/40" />
                    )}
                  </button>
                  <button
                    onClick={() => setExpandedAgent(expandedAgent === agent.id ? null : agent.id)}
                    className="min-w-0 flex-1 text-left"
                  >
                    <p className={cn(
                      "text-[11px] font-mono truncate",
                      agent.status === "active" ? "text-foreground/85" : "text-muted-foreground/40",
                    )}>
                      {agent.name}
                    </p>
                    <p className="text-[9px] font-mono text-muted-foreground/40 truncate">
                      {agent.purpose.slice(0, 50)}
                      {(agent.jobs ?? []).length > 0 ? ` [${(agent.jobs ?? []).length} jobs]` : ""}
                    </p>
                  </button>
                </div>
                <div className="flex items-center gap-1.5 shrink-0 ml-1">
                  <span className="text-[9px] font-mono text-muted-foreground/35">
                    {agent.parsedSchedule?.description ?? agent.schedule}
                  </span>
                  <button
                    onClick={() => handleDelete(agent.id)}
                    className="p-0.5 rounded hover:bg-destructive/15 text-muted-foreground/30 hover:text-destructive transition-colors"
                    title="Delete agent"
                  >
                    <Trash2 size={9} />
                  </button>
                </div>
              </div>

              {/* Expanded details */}
              {expandedAgent === agent.id && (
                <div className="ml-5 mt-0.5 px-2.5 py-1.5 rounded-sm border border-border/12 bg-muted/3 space-y-1">
                  {agent.lastOutput && (
                    <p className="text-[9px] font-mono text-muted-foreground/50 line-clamp-3">
                      Last: {agent.lastOutput.slice(0, 150)}
                    </p>
                  )}
                  {agent.lastRun > 0 && (
                    <p className="text-[9px] font-mono text-muted-foreground/35">
                      Ran {formatTimeAgo(agent.lastRun)}
                    </p>
                  )}
                  {(agent.jobs ?? []).length > 0 && (
                    <div className="space-y-0.5 pt-0.5">
                      <p className="text-[9px] font-mono text-muted-foreground/40 uppercase tracking-wider">Jobs:</p>
                      {(agent.jobs ?? []).map((job) => (
                        <div key={job.id} className="flex items-center justify-between">
                          <span className="text-[9px] font-mono text-foreground/70 truncate">
                            {job.enabled ? "▶" : "⏸"} {job.name}
                          </span>
                          <span className="text-[9px] font-mono text-muted-foreground/35">
                            {job.parsedSchedule?.description ?? job.schedule}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                  {(agent.memory ?? []).length > 0 && (
                    <div className="space-y-0.5 pt-0.5">
                      <p className="text-[9px] font-mono text-muted-foreground/40 uppercase tracking-wider">Memory ({(agent.memory ?? []).length}):</p>
                      {(agent.memory ?? []).slice(-3).map((m, i) => (
                        <p key={i} className="text-[9px] font-mono text-muted-foreground/40 truncate">
                          {m.slice(0, 100)}
                        </p>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Section: Timeline ──────────────────────────────────────────────────────

function Timeline() {
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [recentMemory, setRecentMemory] = useState<ActivityEvent[]>([]);
  const [expanded, setExpanded] = useState(true);

  useEffect(() => {
    // Load persisted entries
    listActivityEntries(30).then(setEntries).catch(() => {});
    // Also get recent from in-memory bus
    setRecentMemory(activityBus.getRecentTasks(20));

    const id = setInterval(() => {
      setRecentMemory(activityBus.getRecentTasks(20));
    }, 5000);
    return () => clearInterval(id);
  }, []);

  // Merge persisted and in-memory, dedup by id, sort by time
  const allItems = (() => {
    const seen = new Set<string>();
    const merged: Array<{
      id: string;
      name: string;
      status: string;
      startedAt: number;
      durationMs?: number;
      detail?: string | null;
    }> = [];

    for (const ev of recentMemory) {
      if (ev.status === "running" || ev.status === "pending") continue; // live tasks shown above
      if (seen.has(ev.id)) continue;
      seen.add(ev.id);
      merged.push({
        id: ev.id,
        name: ev.name,
        status: ev.status,
        startedAt: ev.startedAt,
        durationMs: ev.completedAt ? ev.completedAt - ev.startedAt : undefined,
        detail: ev.detail,
      });
    }
    for (const entry of entries) {
      if (seen.has(entry.id)) continue;
      seen.add(entry.id);
      merged.push({
        id: entry.id,
        name: entry.name,
        status: entry.status,
        startedAt: entry.startedAt,
        durationMs: entry.durationMs,
        detail: entry.detail,
      });
    }

    return merged.sort((a, b) => b.startedAt - a.startedAt).slice(0, 20);
  })();

  return (
    <div className="space-y-1.5">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 text-[10px] font-mono uppercase tracking-wider text-muted-foreground/60 hover:text-muted-foreground/80 transition-colors"
      >
        {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
        Timeline ({allItems.length})
      </button>

      {expanded && (
        <div className="terminal-readout max-h-48 overflow-y-auto space-y-0">
          {allItems.length === 0 ? (
            <p className="text-[10px] font-mono text-muted-foreground/30 text-center py-2">
              No recent activity
            </p>
          ) : (
            allItems.map((item) => (
              <div
                key={item.id}
                className="flex items-start gap-2 px-2 py-1 border-b border-border/10 last:border-0"
              >
                <div className="mt-0.5 shrink-0">
                  {statusIcon(item.status, 9)}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-baseline justify-between gap-2">
                    <p className="text-[10px] font-mono text-foreground/70 truncate">
                      {item.name}
                    </p>
                    <span className="text-[9px] font-mono text-muted-foreground/35 shrink-0 tabular-nums">
                      {formatTimeAgo(item.startedAt)}
                    </span>
                  </div>
                  {item.detail && (
                    <p className="text-[9px] font-mono text-muted-foreground/40 truncate">
                      {item.detail}
                      {item.durationMs ? ` (${formatDuration(item.durationMs)})` : ""}
                    </p>
                  )}
                </div>
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}

// ─── Main Component ─────────────────────────────────────────────────────────

export function ActivityMonitor() {
  const [activeTasks, setActiveTasks] = useState<ActivityEvent[]>([]);

  useEffect(() => {
    // Poll active tasks from the bus
    const refresh = () => setActiveTasks(activityBus.getActiveTasks());
    refresh();
    const pollId = setInterval(refresh, 1000);

    // Also listen for real-time events
    const unlisten = listen<ActivityEvent>("terrance://activity", (event) => {
      setActiveTasks(activityBus.getActiveTasks());
    });

    return () => {
      clearInterval(pollId);
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="heading-section">Activity Monitor</h3>
        <div className="flex items-center gap-1.5">
          {activeTasks.length > 0 && (
            <div className="flex items-center gap-1 px-1.5 py-0.5 rounded-sm bg-accent/10">
              <div className="w-1.5 h-1.5 rounded-full bg-accent animate-pulse" />
              <span className="text-[9px] font-mono text-accent">
                {activeTasks.length} active
              </span>
            </div>
          )}
          <Activity size={11} className="text-muted-foreground/40" />
        </div>
      </div>

      {/* Live Tasks */}
      <LiveTasks tasks={activeTasks} />

      {/* Cron Jobs */}
      <CronJobsSection />

      {/* Subagents */}
      <SubagentsSection />

      {/* Timeline */}
      <Timeline />
    </div>
  );
}
