import { DollarSign, TrendingUp, TrendingDown, Clock, AlertCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ArmRevenueSummary } from "@/lib/types";

const STALE_THRESHOLD_MS = 5 * 60 * 1000;

interface RevenueCardProps {
  revenueSummary: ArmRevenueSummary | null;
  loading?: boolean;
  fetchedAt?: Date | null;
}

export function RevenueCard({ revenueSummary, loading, fetchedAt }: RevenueCardProps) {
  const net = revenueSummary?.net_income ?? null;
  const month = revenueSummary?.month_revenue ?? null;
  const today = revenueSummary?.today_revenue ?? null;
  const isStale = fetchedAt != null && Date.now() - fetchedAt.getTime() > STALE_THRESHOLD_MS;
  const ageLabel = fetchedAt
    ? (() => {
        const secs = Math.round((Date.now() - fetchedAt.getTime()) / 1000);
        if (secs < 60) return `${secs}s ago`;
        const mins = Math.round(secs / 60);
        return `${mins}m ago`;
      })()
    : null;

  return (
    <div className="panel p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="heading-section">Revenue</h3>
        {isStale && (
          <div className="badge-status bg-warning/12 text-warning/80 border border-warning/25">
            <AlertCircle size={8} /> Stale
          </div>
        )}
      </div>

      {loading ? (
        <div className="space-y-2">
          <div className="h-8 bg-muted/15 rounded-sm animate-pulse w-1/2" />
          <div className="h-3 bg-muted/15 rounded-sm animate-pulse w-1/3" />
        </div>
      ) : month != null ? (
        <>
          <div className="flex items-end gap-4">
            <div>
              <p className="metric-value">${month.toFixed(2)}</p>
              <p className="metric-label">This month</p>
            </div>
            {net != null && (
              <div className={cn("flex items-center gap-1 pb-1 text-xs font-mono font-medium",
                net >= 0 ? "text-success/70" : "text-destructive/70"
              )}>
                {net >= 0 ? <TrendingUp size={12} /> : <TrendingDown size={12} />}
                <span>{net >= 0 ? "+" : ""}${net.toFixed(2)} net</span>
              </div>
            )}
          </div>

          {today != null && (
            <div className="flex items-center justify-between px-2.5 py-1.5 rounded-sm bg-muted/5 border border-border/25">
              <span className="metric-label">Today</span>
              <span className="text-xs font-mono font-medium text-success/70">
                +${today.toFixed(2)}
              </span>
            </div>
          )}
        </>
      ) : (
        <div className="p-2.5 rounded-sm bg-muted/5 border border-border/25">
          <p className="text-[10px] font-mono text-muted-foreground/50 text-center uppercase tracking-wider">
            No ARM data — add a strategy
          </p>
        </div>
      )}

      {ageLabel && (
        <div className={cn(
          "flex items-center gap-1 text-[10px] font-mono",
          isStale ? "text-warning/70" : "text-muted-foreground/45",
        )}>
          <Clock size={9} />
          {ageLabel}
        </div>
      )}
    </div>
  );
}
