import { Cpu, Wifi, WifiOff, Clock } from "lucide-react";
import { cn } from "@/lib/utils";
import { formatRelativeTime } from "@/lib/utils";
import type { OpenClawStatus } from "@/lib/types";

interface StatusCardProps {
  status: OpenClawStatus | null;
  loading: boolean;
}

export function StatusCard({ status, loading }: StatusCardProps) {
  const online = status?.online ?? false;

  return (
    <div className="panel-primary p-4 space-y-4">
      {/* Header row */}
      <div className="flex items-center justify-between">
        <h3 className="heading-section">System Core</h3>
        <div
          className={cn(
            "badge-status",
            online
              ? "bg-success/15 text-success border border-success/25 animate-status-glow"
              : "bg-muted/30 text-muted-foreground border border-border/55",
          )}
        >
          {online ? <Wifi size={10} /> : <WifiOff size={10} />}
          {online ? "Online" : "Offline"}
        </div>
      </div>

      {loading ? (
        <div className="space-y-2">
          <div className="h-3 bg-muted/20 rounded-sm animate-pulse w-3/4" />
          <div className="h-3 bg-muted/20 rounded-sm animate-pulse w-1/2" />
        </div>
      ) : (
        <div className="grid grid-cols-3 gap-4">
          {/* Model */}
          <div>
            <p className="metric-label">Model</p>
            <p className="text-sm font-mono font-semibold text-foreground mt-1">
              {status?.model || "—"}
            </p>
          </div>

          {/* Gateway */}
          <div>
            <p className="metric-label">Gateway</p>
            <p className="text-xs font-mono text-muted-foreground mt-1 truncate">
              {status?.gateway_url || "—"}
            </p>
          </div>

          {/* Heartbeat */}
          <div>
            <p className="metric-label">Last Heartbeat</p>
            <p className="text-sm font-mono font-medium text-foreground/85 mt-1">
              {status?.last_heartbeat
                ? formatRelativeTime(status.last_heartbeat)
                : "—"}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
