import { useState, useEffect } from "react";
import { Target, Clock } from "lucide-react";
import { loadTerranceContext } from "@/lib/api";
import { getSchedulerStatus } from "@/terrance/system/scheduler";
import type { SchedulerStatus } from "@/terrance/system/scheduler";

function formatNextRun(lastTs: number | null, intervalMs: number): string {
  if (!lastTs) return "pending";
  const nextTs = lastTs + intervalMs;
  const diff = nextTs - Date.now();
  if (diff <= 0) return "due now";
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ${mins % 60}m`;
}

export function CalendarView() {
  const [goals, setGoals] = useState<string[]>([]);
  const [sched, setSched] = useState<SchedulerStatus | null>(null);

  useEffect(() => {
    loadTerranceContext()
      .then((ctx) => setGoals(ctx.activeGoals ?? []))
      .catch(() => {});
    setSched(getSchedulerStatus());
    const id = setInterval(() => setSched(getSchedulerStatus()), 30_000);
    return () => clearInterval(id);
  }, []);

  const jobs = sched
    ? [
        { name: "Daily Summary", next: formatNextRun(sched.lastDailySummary, 20 * 3600_000) },
        { name: "Consolidation", next: formatNextRun(sched.lastConsolidation, 6 * 3600_000) },
        { name: "Reasoning", next: formatNextRun(sched.lastReasoning, 12 * 3600_000) },
        { name: "Health Check", next: formatNextRun(sched.lastHealthCheck, 30 * 60_000) },
      ]
    : [];

  return (
    <div className="panel p-4 space-y-3">
      <h3 className="heading-section">Today's Focus</h3>

      {goals.length > 0 ? (
        <div className="space-y-1.5">
          {goals.map((g, i) => (
            <div
              key={i}
              className="flex items-start gap-2 text-[13px] text-foreground/85"
            >
              <Target size={12} className="text-accent mt-0.5 shrink-0" />
              <span>{g}</span>
            </div>
          ))}
        </div>
      ) : (
        <p className="text-[11px] text-muted-foreground/50 font-mono">
          No active goals loaded
        </p>
      )}

      {jobs.length > 0 && (
        <>
          <div className="border-t border-border/18 pt-2">
            <p className="text-[10px] font-mono text-muted-foreground/50 uppercase tracking-wider mb-1.5">
              Scheduler
            </p>
            <div className="grid grid-cols-2 gap-x-3 gap-y-1">
              {jobs.map((j) => (
                <div key={j.name} className="flex items-center gap-1.5 text-[11px]">
                  <Clock size={9} className="text-muted-foreground/40" />
                  <span className="text-muted-foreground/60 truncate">{j.name}</span>
                  <span className="text-foreground/70 font-mono ml-auto">{j.next}</span>
                </div>
              ))}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
