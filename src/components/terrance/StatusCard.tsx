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
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <h3 className="text-base font-medium text-muted-foreground">System Status</h3>
        <div
          className={cn(
            "flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium",
            online
              ? "bg-success/15 text-success"
              : "bg-muted/50 text-muted-foreground",
          )}
        >
          {online ? <Wifi size={12} /> : <WifiOff size={12} />}
          {online ? "Online" : "Offline"}
        </div>
      </div>

      {loading ? (
        <div className="space-y-3">
          <div className="h-4 bg-muted/30 rounded animate-pulse w-3/4" />
          <div className="h-4 bg-muted/30 rounded animate-pulse w-1/2" />
        </div>
      ) : (
        <div className="space-y-3">
          <div className="flex items-center gap-3">
            <Cpu size={16} className="text-accent" />
            <div>
              <p className="text-xs text-muted-foreground">Model</p>
              <p className="text-sm font-medium">{status?.model || "Unknown"}</p>
            </div>
          </div>

          <div className="flex items-center gap-3">
            <Wifi size={16} className="text-accent" />
            <div>
              <p className="text-xs text-muted-foreground">Gateway</p>
              <p className="text-sm font-mono text-muted-foreground">
                {status?.gateway_url || "—"}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-3">
            <Clock size={16} className="text-accent" />
            <div>
              <p className="text-xs text-muted-foreground">Last Heartbeat</p>
              <p className="text-sm font-medium">
                {status?.last_heartbeat
                  ? formatRelativeTime(status.last_heartbeat)
                  : "No data"}
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
