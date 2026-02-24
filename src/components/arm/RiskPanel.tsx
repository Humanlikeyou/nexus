import { useState } from "react";
import { ShieldAlert, AlertTriangle, Zap } from "lucide-react";
import { cn } from "@/lib/utils";
import * as Dialog from "@radix-ui/react-dialog";
import type { ArmRiskStatus } from "@/lib/types";

interface RiskPanelProps {
  riskStatus: ArmRiskStatus | null;
  onKillSwitch: (active: boolean, reasoning: string) => void;
}

export function RiskPanel({ riskStatus, onKillSwitch }: RiskPanelProps) {
  const [killDialogOpen, setKillDialogOpen] = useState(false);
  const [killConfirm, setKillConfirm] = useState("");

  const risk = riskStatus;
  const riskIndex = risk?.risk_index ?? 0;
  const capitalPct =
    risk && risk.global_capital_limit > 0
      ? (risk.global_capital_deployed / risk.global_capital_limit) * 100
      : 0;
  const lossPct =
    risk && risk.global_daily_loss_cap > 0
      ? (Math.abs(risk.global_daily_pnl) / risk.global_daily_loss_cap) * 100
      : 0;

  const riskColor =
    riskIndex < 0.3
      ? "text-success"
      : riskIndex < 0.6
        ? "text-warning"
        : "text-destructive";

  const handleKillSwitch = () => {
    if (risk?.kill_switch_active) {
      onKillSwitch(false, "Manual deactivation of kill switch");
      return;
    }
    setKillDialogOpen(true);
  };

  const confirmKill = () => {
    if (killConfirm !== "KILL") return;
    onKillSwitch(true, "Manual kill switch activation");
    setKillDialogOpen(false);
    setKillConfirm("");
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <ShieldAlert size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">
            Risk Controls
          </h3>
        </div>
        {risk?.strategies_at_risk && risk.strategies_at_risk.length > 0 && (
          <div className="flex items-center gap-1 text-warning">
            <AlertTriangle size={12} />
            <span className="text-xs">
              {risk.strategies_at_risk.length} at risk
            </span>
          </div>
        )}
      </div>

      {/* Risk Index */}
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground">Risk Index</span>
        <span className={cn("text-lg font-mono font-semibold", riskColor)}>
          {(riskIndex * 100).toFixed(0)}%
        </span>
      </div>

      {/* Capital Deployed */}
      <div className="space-y-1">
        <div className="flex items-center justify-between">
          <span className="text-[13px] text-muted-foreground/60">
            Capital Deployed
          </span>
          <span className="text-[13px] font-mono text-muted-foreground">
            ${risk?.global_capital_deployed?.toFixed(2) ?? "0.00"} / $
            {risk?.global_capital_limit?.toFixed(2) ?? "0.00"}
          </span>
        </div>
        <div className="w-full h-2 rounded-full bg-muted/30 overflow-hidden">
          <div
            className={cn(
              "h-full rounded-full transition-all duration-500",
              capitalPct > 80 ? "bg-destructive" : capitalPct > 50 ? "bg-warning" : "bg-accent",
            )}
            style={{ width: `${Math.min(100, capitalPct)}%` }}
          />
        </div>
      </div>

      {/* Daily P&L */}
      <div className="space-y-1">
        <div className="flex items-center justify-between">
          <span className="text-[13px] text-muted-foreground/60">
            Daily P&L
          </span>
          <span
            className={cn(
              "text-[13px] font-mono",
              (risk?.global_daily_pnl ?? 0) >= 0
                ? "text-success"
                : "text-destructive",
            )}
          >
            {(risk?.global_daily_pnl ?? 0) >= 0 ? "+" : ""}$
            {risk?.global_daily_pnl?.toFixed(2) ?? "0.00"} / -$
            {risk?.global_daily_loss_cap?.toFixed(2) ?? "0.00"}
          </span>
        </div>
        <div className="w-full h-2 rounded-full bg-muted/30 overflow-hidden">
          <div
            className={cn(
              "h-full rounded-full transition-all duration-500",
              lossPct > 80 ? "bg-destructive" : lossPct > 50 ? "bg-warning" : "bg-success",
            )}
            style={{ width: `${Math.min(100, lossPct)}%` }}
          />
        </div>
      </div>

      {/* Stats Row */}
      <div className="flex items-center gap-4 pt-1">
        <div>
          <p className="text-xs text-muted-foreground/40">Active</p>
          <p className="text-sm font-mono">{risk?.active_strategies ?? 0}</p>
        </div>
        <div>
          <p className="text-xs text-muted-foreground/40">Kill Switch</p>
          <p
            className={cn(
              "text-sm font-mono",
              risk?.kill_switch_active ? "text-destructive" : "text-success",
            )}
          >
            {risk?.kill_switch_active ? "ACTIVE" : "OFF"}
          </p>
        </div>
      </div>

      {/* Kill Switch Button */}
      <button
        onClick={handleKillSwitch}
        className={cn(
          "w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-xs font-medium transition-colors",
          risk?.kill_switch_active
            ? "bg-success/15 text-success hover:bg-success/25 border border-success/20"
            : "bg-destructive/10 text-destructive hover:bg-destructive/20 border border-destructive/20",
        )}
      >
        <Zap size={14} />
        {risk?.kill_switch_active ? "Deactivate Kill Switch" : "Kill Switch"}
      </button>

      {/* Kill Confirmation Dialog */}
      <Dialog.Root
        open={killDialogOpen}
        onOpenChange={(open) => {
          if (!open) {
            setKillDialogOpen(false);
            setKillConfirm("");
          }
        }}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-[85] bg-black/50 backdrop-blur-sm" />
          <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[86] w-[400px] glass rounded-xl border border-destructive/30 shadow-2xl p-6 space-y-5">
            <Dialog.Title className="text-sm font-semibold text-destructive">
              Activate Kill Switch
            </Dialog.Title>
            <p className="text-xs text-muted-foreground">
              This will immediately pause all active strategies and send kill
              signals to all workers. Type <strong>KILL</strong> to confirm.
            </p>
            <input
              type="text"
              value={killConfirm}
              onChange={(e) => setKillConfirm(e.target.value)}
              placeholder='Type "KILL" to confirm'
              className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-destructive/30 text-sm font-mono placeholder:text-muted-foreground/20 focus:outline-none focus:border-destructive/60 transition-colors"
              autoFocus
            />
            <div className="flex items-center justify-end gap-3">
              <Dialog.Close asChild>
                <button className="px-3 py-2 rounded-lg text-sm text-muted-foreground hover:bg-muted/20 transition-colors">
                  Cancel
                </button>
              </Dialog.Close>
              <button
                onClick={confirmKill}
                disabled={killConfirm !== "KILL"}
                className="px-4 py-2 rounded-lg bg-destructive/15 text-destructive text-sm font-medium hover:bg-destructive/25 transition-colors disabled:opacity-40"
              >
                Activate Kill Switch
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  );
}
