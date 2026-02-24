import { Clock, Play, Pause } from "lucide-react";
import { cn } from "@/lib/utils";
import type { CronJob } from "@/lib/types";

interface CronScheduleProps {
  jobs: CronJob[];
  loading: boolean;
}

const defaultJobs: CronJob[] = [
  { name: "Morning Briefing", schedule: "0 7 * * *", next_run: null, last_run: null, enabled: true },
  { name: "Midday Check-in", schedule: "0 12 * * *", next_run: null, last_run: null, enabled: true },
  { name: "Evening Wrap-up", schedule: "0 21 * * *", next_run: null, last_run: null, enabled: true },
  { name: "Weekly Review", schedule: "0 10 * * 0", next_run: null, last_run: null, enabled: false },
];

export function CronSchedule({ jobs, loading }: CronScheduleProps) {
  const displayJobs = jobs.length > 0 ? jobs : defaultJobs;

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center gap-2">
        <Clock size={16} className="text-accent" />
        <h3 className="text-base font-medium text-muted-foreground">Cron Schedule</h3>
      </div>

      {loading ? (
        <div className="space-y-2">
          {[1, 2, 3].map((i) => (
            <div key={i} className="h-10 bg-muted/30 rounded animate-pulse" />
          ))}
        </div>
      ) : (
        <div className="space-y-2">
          {displayJobs.map((job, i) => (
            <div
              key={i}
              className={cn(
                "flex items-center justify-between px-3 py-2.5 rounded-lg",
                "bg-muted/20 border border-border/20",
              )}
            >
              <div className="flex items-center gap-3">
                {job.enabled ? (
                  <Play size={12} className="text-success" />
                ) : (
                  <Pause size={12} className="text-muted-foreground/50" />
                )}
                <div>
                  <p className={cn("text-sm", !job.enabled && "text-muted-foreground/50")}>
                    {job.name}
                  </p>
                  <p className="text-xs font-mono text-muted-foreground/60">
                    {job.schedule}
                  </p>
                </div>
              </div>
              {job.next_run && (
                <span className="text-xs text-muted-foreground">{job.next_run}</span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
