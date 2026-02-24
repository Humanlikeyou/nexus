import { DollarSign, TrendingUp, TrendingDown } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ArmRevenueSummary } from "@/lib/types";

interface RevenueCardProps {
  revenueSummary: ArmRevenueSummary | null;
  loading?: boolean;
}

export function RevenueCard({ revenueSummary, loading }: RevenueCardProps) {
  const net = revenueSummary?.net_income ?? null;
  const month = revenueSummary?.month_revenue ?? null;
  const today = revenueSummary?.today_revenue ?? null;

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center gap-2">
        <DollarSign size={16} className="text-success" />
        <h3 className="text-base font-medium text-muted-foreground">Revenue</h3>
      </div>

      {loading ? (
        <div className="space-y-3">
          <div className="h-10 bg-muted/30 rounded animate-pulse w-1/2" />
          <div className="h-4 bg-muted/30 rounded animate-pulse w-1/3" />
        </div>
      ) : month != null ? (
        <>
          <div className="flex items-end gap-4">
            <div>
              <p className="text-3xl font-semibold">${month.toFixed(2)}</p>
              <p className="text-xs text-muted-foreground">This month</p>
            </div>
            {net != null && (
              <div className={cn("flex items-center gap-1 pb-1 text-sm font-medium",
                net >= 0 ? "text-success" : "text-destructive"
              )}>
                {net >= 0 ? <TrendingUp size={14} /> : <TrendingDown size={14} />}
                <span>{net >= 0 ? "+" : ""}${net.toFixed(2)} net</span>
              </div>
            )}
          </div>

          {today != null && (
            <div className="flex items-center justify-between px-3 py-2 rounded-lg bg-muted/10 border border-border/10">
              <span className="text-xs text-muted-foreground/60">Today</span>
              <span className="text-xs font-mono font-medium text-success">
                +${today.toFixed(2)}
              </span>
            </div>
          )}
        </>
      ) : (
        <div className="p-3 rounded-lg bg-muted/10 border border-border/10">
          <p className="text-xs text-muted-foreground/60 text-center">
            No ARM revenue data — add a strategy on the Revenue page
          </p>
        </div>
      )}
    </div>
  );
}
