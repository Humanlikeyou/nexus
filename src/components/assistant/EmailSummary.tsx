import { useState, useEffect } from "react";
import { Brain, MessageSquare } from "lucide-react";
import { listSessionLogs, loadTerranceContext } from "@/lib/api";
import type { SessionLogEntry } from "@/lib/types";

function timeAgo(ts: number): string {
  const mins = Math.floor((Date.now() - ts) / 60000);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

export function EmailSummary() {
  const [logs, setLogs] = useState<SessionLogEntry[]>([]);
  const [memory, setMemory] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([listSessionLogs(), loadTerranceContext()])
      .then(([allLogs, ctx]) => {
        // Show last 4 non-system logs (exclude heal/consolidated/daily-summary)
        const userLogs = allLogs.filter(
          (l) =>
            !l.id.startsWith("heal-") &&
            !l.id.startsWith("consolidated-") &&
            !l.id.startsWith("daily-summary-"),
        );
        setLogs(userLogs.slice(0, 4));
        setMemory((ctx.relevantMemory ?? []).slice(0, 5));
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="panel p-4 space-y-3">
        <h3 className="heading-section">Recent Activity</h3>
        <div className="space-y-1.5">
          <div className="h-3 bg-muted/10 rounded-sm animate-pulse w-3/4" />
          <div className="h-3 bg-muted/10 rounded-sm animate-pulse w-full" />
          <div className="h-3 bg-muted/10 rounded-sm animate-pulse w-5/6" />
        </div>
      </div>
    );
  }

  return (
    <div className="panel p-4 space-y-3">
      <h3 className="heading-section">Recent Activity</h3>

      {logs.length > 0 ? (
        <div className="space-y-2">
          {logs.map((l) => (
            <div key={l.id} className="space-y-0.5">
              <div className="flex items-center gap-1.5">
                <MessageSquare size={10} className="text-accent/60 shrink-0" />
                <span className="text-[10px] font-mono text-muted-foreground/50">
                  {timeAgo(l.timestamp)}
                </span>
              </div>
              <p className="text-[12px] text-foreground/75 leading-snug line-clamp-2 pl-4">
                {l.summary.replace(/^#+\s*/gm, "").replace(/\*\*/g, "").slice(0, 140)}
              </p>
            </div>
          ))}
        </div>
      ) : (
        <p className="text-[11px] text-muted-foreground/50 font-mono">No recent sessions</p>
      )}

      {memory.length > 0 && (
        <div className="border-t border-border/18 pt-2">
          <p className="text-[10px] font-mono text-muted-foreground/50 uppercase tracking-wider mb-1.5">
            Active Memory
          </p>
          <div className="space-y-1">
            {memory.map((m, i) => (
              <div key={i} className="flex items-start gap-1.5 text-[11px] text-foreground/65 leading-snug">
                <Brain size={9} className="text-accent/40 mt-0.5 shrink-0" />
                <span className="line-clamp-1">{m}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
