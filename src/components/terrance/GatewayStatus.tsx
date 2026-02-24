import { useState, useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ArrowUpRight, Clock, AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import { getOutboxStatus } from "@/lib/api";
import type { OutboxStatus } from "@/lib/types";

function relativeTime(isoStr: string | null): string {
  if (!isoStr) return "never";
  const diff = Date.now() - new Date(isoStr).getTime();
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ago`;
}

export function GatewayStatus() {
  const [status, setStatus] = useState<OutboxStatus | null>(null);

  useEffect(() => {
    // Initial fetch
    getOutboxStatus().then(setStatus).catch(() => {});

    // Live updates from delivery worker
    let unlisten: UnlistenFn | null = null;
    listen<OutboxStatus>("outbox-status", (event) => {
      setStatus(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    // Poll every 30s as fallback
    const interval = setInterval(() => {
      getOutboxStatus().then(setStatus).catch(() => {});
    }, 30000);

    return () => {
      unlisten?.();
      clearInterval(interval);
    };
  }, []);

  if (!status) {
    return (
      <div className="glass p-6">
        <div className="flex items-center gap-2 text-xs text-muted-foreground/50">
          <ArrowUpRight size={14} />
          <span>Gateway Pipeline</span>
          <span className="ml-auto text-xs">Loading...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="glass p-6 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <ArrowUpRight size={14} className="text-accent" />
          <span className="text-xs font-medium text-muted-foreground">Gateway Pipeline</span>
        </div>
        <div className="flex items-center gap-1.5">
          <div
            className={cn(
              "w-2 h-2 rounded-full",
              status.gateway_online
                ? "bg-success animate-pulse-dot"
                : "bg-destructive",
            )}
          />
          <span className="text-xs text-muted-foreground">
            {status.gateway_online ? "Online" : "Offline"}
          </span>
        </div>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <div>
          <p className="text-xs text-muted-foreground/50">Queue</p>
          <p className={cn(
            "text-sm font-mono font-semibold",
            status.queue_depth > 0 ? "text-warning" : "text-foreground/70",
          )}>
            {status.queue_depth}
          </p>
        </div>
        <div>
          <p className="text-xs text-muted-foreground/50">Delivered</p>
          <p className="text-sm font-mono font-semibold text-foreground/70">
            {status.total_delivered}
          </p>
        </div>
        <div>
          <div className="flex items-center gap-1">
            <Clock size={8} className="text-muted-foreground/40" />
            <p className="text-xs text-muted-foreground/50">Last Sent</p>
          </div>
          <p className="text-[13px] font-mono text-foreground/70">
            {relativeTime(status.last_delivered_at)}
          </p>
        </div>
      </div>

      {status.last_error && !status.gateway_online && (
        <div className="flex items-start gap-1.5 px-2.5 py-1.5 rounded-md bg-destructive/5 border border-destructive/15">
          <AlertTriangle size={10} className="text-destructive mt-0.5 shrink-0" />
          <p className="text-xs text-destructive/80 break-all">{status.last_error}</p>
        </div>
      )}
    </div>
  );
}
