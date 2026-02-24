import { useState } from "react";
import { DollarSign, RefreshCw, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { StrategyList } from "./StrategyList";
import { RiskPanel } from "./RiskPanel";
import { RevenueChart } from "./RevenueChart";
import { DecisionLog } from "./DecisionLog";
import { StrategyDetail } from "./StrategyDetail";
import type {
  ArmStrategy,
  ArmRevenueSummary,
  ArmRiskStatus,
  ArmDecision,
  ArmConfig,
  ArmStrategyConfig,
} from "@/lib/types";

interface ArmDashboardProps {
  strategies: ArmStrategy[];
  revenueSummary: ArmRevenueSummary | null;
  riskStatus: ArmRiskStatus | null;
  decisions: ArmDecision[];
  config: ArmConfig | null;
  loading: boolean;
  error: string | null;
  polling: boolean;
  onRefresh: () => void;
  onPollWorkers: () => void;
  onCreateStrategy: (
    name: string,
    type: string,
    config: ArmStrategyConfig,
    capitalLimit: number,
    dailyLossCap: number,
  ) => void;
  onUpdateStrategy: (
    id: string,
    name?: string,
    config?: ArmStrategyConfig,
    capitalLimit?: number,
    dailyLossCap?: number,
  ) => Promise<ArmStrategy>;
  onDeleteStrategy: (id: string) => void;
  onActivate: (id: string) => void;
  onPause: (id: string, reasoning: string) => void;
  onKill: (id: string, reasoning: string) => void;
  onKillSwitch: (active: boolean, reasoning: string) => void;
  onLogDecision: (
    strategyId: string,
    action: string,
    reasoning: string,
    context?: Record<string, unknown>,
  ) => void;
  onUpdateOutcome: (id: string, outcome: string, notes?: string) => void;
}

export function ArmDashboard({
  strategies,
  revenueSummary,
  riskStatus,
  decisions,
  config: _config,
  loading,
  error,
  polling,
  onRefresh,
  onPollWorkers,
  onCreateStrategy,
  onUpdateStrategy,
  onDeleteStrategy,
  onActivate,
  onPause,
  onKill,
  onKillSwitch,
  onLogDecision: _onLogDecision,
  onUpdateOutcome,
}: ArmDashboardProps) {
  const [selectedStrategy, setSelectedStrategy] = useState<ArmStrategy | null>(null);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 size={24} className="text-accent animate-spin" />
      </div>
    );
  }

  if (selectedStrategy) {
    const current = strategies.find((s) => s.id === selectedStrategy.id) ?? selectedStrategy;
    return (
      <StrategyDetail
        strategy={current}
        decisions={decisions.filter((d) => d.strategy_id === current.id)}
        onBack={() => setSelectedStrategy(null)}
        onActivate={onActivate}
        onPause={onPause}
        onKill={onKill}
        onUpdate={onUpdateStrategy}
        onDelete={onDeleteStrategy}
      />
    );
  }

  const totalNet = revenueSummary
    ? revenueSummary.total_revenue - revenueSummary.total_expenses
    : 0;

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-success/15 flex items-center justify-center">
            <DollarSign size={16} className="text-success" />
          </div>
          <div>
            <h2 className="text-sm font-semibold">Autonomous Revenue Module</h2>
            <p className="text-xs text-muted-foreground/50">
              {strategies.filter((s) => s.status === "active").length} active strategies ·{" "}
              <span className={cn(totalNet >= 0 ? "text-success" : "text-destructive")}>
                {totalNet >= 0 ? "+" : ""}${totalNet.toFixed(2)} net
              </span>
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={onPollWorkers}
            disabled={polling}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs transition-colors",
              polling
                ? "bg-muted/20 text-muted-foreground cursor-wait"
                : "bg-accent/15 text-accent hover:bg-accent/25",
            )}
          >
            {polling ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <RefreshCw size={12} />
            )}
            {polling ? "Polling..." : "Poll Workers"}
          </button>
        </div>
      </div>

      {error && (
        <div className="px-4 py-3 rounded-lg bg-destructive/10 border border-destructive/20">
          <p className="text-xs text-destructive">{error}</p>
        </div>
      )}

      {/* Row 1: Revenue Chart + Risk */}
      <div className="grid grid-cols-3 gap-4">
        <div className="col-span-2">
          <RevenueChart revenueSummary={revenueSummary} />
        </div>
        <RiskPanel riskStatus={riskStatus} onKillSwitch={onKillSwitch} />
      </div>

      {/* Row 2: Strategies + Decision Log */}
      <div className="grid grid-cols-2 gap-4">
        <StrategyList
          strategies={strategies}
          onActivate={onActivate}
          onPause={onPause}
          onKill={onKill}
          onCreate={onCreateStrategy}
          onSelect={setSelectedStrategy}
        />
        <DecisionLog
          decisions={decisions}
          strategies={strategies.map((s) => ({ id: s.id, name: s.name }))}
          onUpdateOutcome={onUpdateOutcome}
        />
      </div>
    </div>
  );
}
