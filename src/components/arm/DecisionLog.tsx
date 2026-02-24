import { BookOpen, ArrowUpCircle, PauseCircle, XCircle, Settings2, Rocket, TrendingUp, RotateCcw } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ArmDecision, ArmDecisionAction } from "@/lib/types";

const actionConfig: Record<
  ArmDecisionAction,
  { icon: typeof ArrowUpCircle; color: string; bg: string }
> = {
  activate: { icon: ArrowUpCircle, color: "text-success", bg: "bg-success/15" },
  pause: { icon: PauseCircle, color: "text-warning", bg: "bg-warning/15" },
  kill: { icon: XCircle, color: "text-destructive", bg: "bg-destructive/15" },
  adjust_limit: { icon: Settings2, color: "text-accent", bg: "bg-accent/15" },
  deploy: { icon: Rocket, color: "text-accent", bg: "bg-accent/15" },
  scale: { icon: TrendingUp, color: "text-success", bg: "bg-success/15" },
  rollback: { icon: RotateCcw, color: "text-warning", bg: "bg-warning/15" },
};

const outcomeColors: Record<string, string> = {
  positive: "text-success",
  negative: "text-destructive",
  neutral: "text-muted-foreground",
};

interface DecisionLogProps {
  decisions: ArmDecision[];
  strategies: Array<{ id: string; name: string }>;
  onUpdateOutcome?: (id: string, outcome: string, notes?: string) => void;
}

function formatRelativeTime(timestamp: string): string {
  const diff = Date.now() - new Date(timestamp).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function DecisionLog({
  decisions,
  strategies,
}: DecisionLogProps) {
  const stratMap = new Map(strategies.map((s) => [s.id, s.name]));

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <BookOpen size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">
            Decision Journal
          </h3>
          <span className="text-xs text-muted-foreground/40">
            {decisions.length} entries
          </span>
        </div>
      </div>

      {decisions.length === 0 ? (
        <div className="py-8 text-center">
          <BookOpen
            size={24}
            className="text-muted-foreground/20 mx-auto mb-2"
          />
          <p className="text-xs text-muted-foreground/40">
            No decisions logged yet.
          </p>
        </div>
      ) : (
        <div className="space-y-2 max-h-[320px] overflow-y-auto pr-1">
          {decisions.slice(0, 30).map((dec) => {
            const cfg = actionConfig[dec.action] ?? actionConfig.activate;
            const Icon = cfg.icon;
            const stratName = stratMap.get(dec.strategy_id) ?? dec.strategy_id.slice(0, 8);

            return (
              <div
                key={dec.id}
                className="px-3 py-2.5 rounded-lg bg-muted/10 border border-border/10 space-y-1"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div
                      className={cn(
                        "p-1 rounded",
                        cfg.bg,
                      )}
                    >
                      <Icon size={12} className={cfg.color} />
                    </div>
                    <span className="text-xs font-medium">
                      {dec.action.replace(/_/g, " ")}
                    </span>
                    <span className="text-xs text-muted-foreground/40">
                      {stratName}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    {dec.outcome && (
                      <span
                        className={cn(
                          "text-xs font-medium",
                          outcomeColors[dec.outcome] ?? "text-muted-foreground",
                        )}
                      >
                        {dec.outcome}
                      </span>
                    )}
                    <span className="text-xs text-muted-foreground/30">
                      {formatRelativeTime(dec.timestamp)}
                    </span>
                  </div>
                </div>
                <p className="text-[13px] text-muted-foreground/60 leading-relaxed">
                  {dec.reasoning}
                </p>
                {dec.outcome_notes && (
                  <p className="text-xs text-muted-foreground/40 italic">
                    {dec.outcome_notes}
                  </p>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
