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
    getOutboxStatus().then(setStatus).catch(() => {});

    let unlisten: UnlistenFn | null = null;
    listen<OutboxStatus>("outbox-status", (event) => {
      setStatus(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

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
      <div className="panel p-4">
        <div className="flex items-center gap-2">
          <ArrowUpRight size={12} className="text-muted-foreground/50" />
          <span className="heading-section">Gateway Pipeline</span>
          <span className="ml-auto text-[10px] font-mono text-muted-foreground/50">Loading...</span>
        </div>
      </div>
    );
  }

  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="heading-section">Gateway Pipeline</h3>
        <div className="flex items-center gap-1.5">
          <div
            className={cn(
              "w-1.5 h-1.5 rounded-full",
              status.gateway_online
                ? "bg-success animate-pulse-dot"
                : "bg-destructive",
            )}
          />
          <span className="text-[10px] font-mono text-muted-foreground/70 uppercase tracking-wider">
            {status.gateway_online ? "Online" : "Offline"}
          </span>
        </div>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <div>
          <p className="metric-label">Queue</p>
          <p className={cn(
            "text-sm font-mono font-semibold mt-0.5",
            status.queue_depth > 0 ? "text-warning" : "text-foreground/75",
          )}>
            {status.queue_depth}
          </p>
        </div>
        <div>
          <p className="metric-label">Delivered</p>
          <p className="text-sm font-mono font-semibold text-foreground/75 mt-0.5">
            {status.total_delivered}
          </p>
        </div>
        <div>
          <p className="metric-label">Last Sent</p>
          <p className="text-xs font-mono text-foreground/75 mt-0.5">
            {relativeTime(status.last_delivered_at)}
          </p>
        </div>
      </div>

      {status.last_error && !status.gateway_online && (
        <div className="flex items-start gap-1.5 px-2 py-1.5 rounded-sm bg-destructive/8 border border-destructive/20">
          <AlertTriangle size={9} className="text-destructive/80 mt-0.5 shrink-0" />
          <p className="text-[10px] font-mono text-destructive/80 break-all">{status.last_error}</p>
        </div>
      )}
    </div>
  );
}
