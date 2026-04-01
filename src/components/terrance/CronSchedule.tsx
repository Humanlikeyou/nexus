import { useState, useEffect } from "react";
import { Play, Pause } from "lucide-react";
import { cn } from "@/lib/utils";
import type { CronJob } from "@/lib/types";
import { getSchedulerStatus } from "@/terrance/system/scheduler";
import type { SchedulerStatus } from "@/terrance/system/scheduler";

interface CronScheduleProps {
  jobs: CronJob[];
  loading: boolean;
}

function formatNextRun(lastRun: number | null, intervalMs: number): string {
  if (!lastRun) return "pending";
  const next = lastRun + intervalMs;
  const now = Date.now();
  const diffMs = next - now;
  if (diffMs <= 0) return "due now";
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) return `in ${diffMin}m`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `in ${diffHr}h ${diffMin % 60}m`;
  return `in ${Math.floor(diffHr / 24)}d`;
}

function formatLastRun(ts: number | null): string {
  if (!ts) return "never";
  const diffMs = Date.now() - ts;
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  return `${Math.floor(diffHr / 24)}d ago`;
}

interface SchedulerJob {
  name: string;
  interval: string;
  lastRun: number | null;
  intervalMs: number;
  enabled: boolean;
}

function buildSchedulerJobs(status: SchedulerStatus): SchedulerJob[] {
  return [
    {
      name: "Daily Summary",
      interval: `daily @ ${status.dailySummaryHour}:00`,
      lastRun: status.lastDailySummary,
      intervalMs: 24 * 60 * 60 * 1000,
      enabled: true,
    },
    {
      name: "Health Check",
      interval: "every 30m",
      lastRun: status.lastHealthCheck,
      intervalMs: 30 * 60 * 1000,
      enabled: true,
    },
    {
      name: "Consolidation",
      interval: "every 6h",
      lastRun: status.lastConsolidation,
      intervalMs: 6 * 60 * 60 * 1000,
      enabled: true,
    },
    {
      name: "Reasoning Loop",
      interval: "every 12h",
      lastRun: status.lastReasoning,
      intervalMs: 12 * 60 * 60 * 1000,
      enabled: true,
    },
    {
      name: "Memory Pruning",
      interval: "weekly",
      lastRun: status.lastPrune,
      intervalMs: 7 * 24 * 60 * 60 * 1000,
      enabled: true,
    },
  ];
}

export function CronSchedule({ jobs, loading }: CronScheduleProps) {
  const [status, setStatus] = useState<SchedulerStatus | null>(null);

  useEffect(() => {
    const refresh = () => setStatus(getSchedulerStatus());
    refresh();
    const id = setInterval(refresh, 30_000); // refresh every 30s
    return () => clearInterval(id);
  }, []);

  const schedulerJobs = status ? buildSchedulerJobs(status) : [];
  const externalJobs = (jobs ?? []).length > 0 ? jobs : [];

  return (
    <div className="panel p-4 space-y-3">
      <h3 className="heading-section">Schedule</h3>

      {loading && !status ? (
        <div className="space-y-1.5">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-8 bg-muted/15 rounded-sm animate-pulse" />
          ))}
        </div>
      ) : (
        <div className="space-y-1">
          {/* Scheduler-managed jobs */}
          {schedulerJobs.map((job, i) => (
            <div
              key={`sched-${i}`}
              className="flex items-center justify-between px-2.5 py-2 rounded-sm border border-border/35 bg-muted/8"
            >
              <div className="flex items-center gap-2.5">
                {job.enabled ? (
                  <Play size={9} className="text-success" />
                ) : (
                  <Pause size={9} className="text-muted-foreground/50" />
                )}
                <div>
                  <p className={cn(
                    "text-[11px] font-mono",
                    job.enabled ? "text-foreground/85" : "text-muted-foreground/50"
                  )}>
                    {job.name}
                  </p>
                  <p className="text-[10px] font-mono text-muted-foreground/50">
                    {job.interval}
                  </p>
                </div>
              </div>
              <div className="text-right">
                <p className="text-[10px] font-mono text-muted-foreground/50">
                  {formatNextRun(job.lastRun, job.intervalMs)}
                </p>
                <p className="text-[9px] font-mono text-muted-foreground/35">
                  {formatLastRun(job.lastRun)}
                </p>
              </div>
            </div>
          ))}

          {/* External OpenClaw cron jobs */}
          {externalJobs.map((job, i) => (
            <div
              key={`ext-${i}`}
              className="flex items-center justify-between px-2.5 py-2 rounded-sm border border-border/35 bg-muted/8"
            >
              <div className="flex items-center gap-2.5">
                {job.enabled ? (
                  <Play size={9} className="text-success" />
                ) : (
                  <Pause size={9} className="text-muted-foreground/50" />
                )}
                <div>
                  <p className={cn(
                    "text-[11px] font-mono",
                    job.enabled ? "text-foreground/85" : "text-muted-foreground/50"
                  )}>
                    {job.name}
                  </p>
                  <p className="text-[10px] font-mono text-muted-foreground/50">
                    {job.schedule}
                  </p>
                </div>
              </div>
              {job.next_run && (
                <span className="text-[10px] font-mono text-muted-foreground/50">{job.next_run}</span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
