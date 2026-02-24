import { Coins } from "lucide-react";

interface TokenUsageProps {
  loading: boolean;
}

export function TokenUsage({ loading }: TokenUsageProps) {
  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Coins size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">Token Usage</h3>
        </div>
        <span className="text-xs text-muted-foreground">This week</span>
      </div>

      <div className="flex gap-6">
        <div>
          <p className="text-2xl font-semibold text-muted-foreground/40">--</p>
          <p className="text-xs text-muted-foreground">Total tokens</p>
        </div>
        <div>
          <p className="text-2xl font-semibold text-muted-foreground/40">--</p>
          <p className="text-xs text-muted-foreground">Est. cost</p>
        </div>
      </div>

      {loading ? (
        <div className="h-[120px] bg-muted/30 rounded animate-pulse" />
      ) : (
        <div className="h-[120px] flex items-center justify-center">
          <span className="text-xs text-muted-foreground/40">
            Token tracking not yet connected
          </span>
        </div>
      )}
    </div>
  );
}
