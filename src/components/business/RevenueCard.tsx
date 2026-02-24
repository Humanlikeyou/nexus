import { DollarSign, TrendingUp } from "lucide-react";

export function RevenueCard() {
  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center gap-2">
        <DollarSign size={16} className="text-success" />
        <h3 className="text-base font-medium text-muted-foreground">Revenue</h3>
      </div>

      <div className="flex items-end gap-4">
        <div>
          <p className="text-3xl font-semibold">$0</p>
          <p className="text-xs text-muted-foreground">This month</p>
        </div>
        <div className="flex items-center gap-1 text-muted-foreground/50 pb-1">
          <TrendingUp size={12} />
          <span className="text-xs">—</span>
        </div>
      </div>

      <div className="p-3 rounded-lg bg-muted/10 border border-border/10">
        <p className="text-xs text-muted-foreground/60 text-center">
          Connect Stripe to see live revenue metrics
        </p>
      </div>
    </div>
  );
}
