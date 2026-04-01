import { useState } from "react";
import {
  ArrowLeft,
  Play,
  Pause,
  Skull,
  Trash2,
  Edit3,
  Circle,
  ExternalLink,
} from "lucide-react";
import { cn } from "@/lib/utils";
import * as Dialog from "@radix-ui/react-dialog";
import type { ArmStrategy, ArmDecision, ArmStrategyConfig } from "@/lib/types";

const statusColors: Record<string, string> = {
  draft: "text-muted-foreground",
  testing: "text-warning",
  active: "text-success",
  paused: "text-orange-400",
  killed: "text-destructive",
};

const healthColors: Record<string, string> = {
  healthy: "text-success",
  degraded: "text-warning",
  down: "text-destructive",
  unknown: "text-muted-foreground/50",
};

interface StrategyDetailProps {
  strategy: ArmStrategy;
  decisions: ArmDecision[];
  onBack: () => void;
  onActivate: (id: string) => void;
  onPause: (id: string, reasoning: string) => void;
  onKill: (id: string, reasoning: string) => void;
  onUpdate: (
    id: string,
    name?: string,
    config?: ArmStrategyConfig,
    capitalLimit?: number,
    dailyLossCap?: number,
  ) => Promise<ArmStrategy>;
  onDelete: (id: string) => void;
}

export function StrategyDetail({
  strategy,
  decisions,
  onBack,
  onActivate,
  onPause,
  onKill,
  onUpdate,
  onDelete,
}: StrategyDetailProps) {
  const [editOpen, setEditOpen] = useState(false);
  const [editName, setEditName] = useState(strategy.name);
  const [editCapital, setEditCapital] = useState(strategy.capital_limit);
  const [editLossCap, setEditLossCap] = useState(strategy.daily_loss_cap);
  const [reasonDialog, setReasonDialog] = useState<"pause" | "kill" | null>(null);
  const [reason, setReason] = useState("");
  const [deleteConfirm, setDeleteConfirm] = useState(false);
  const [saving, setSaving] = useState(false);
  const [actioning, setActioning] = useState(false);

  const net = strategy.total_revenue - strategy.total_expenses;

  const handleSaveEdit = async () => {
    if (!editName.trim() || saving) return;
    setSaving(true);
    try {
      await onUpdate(strategy.id, editName, undefined, editCapital, editLossCap);
      setEditOpen(false);
    } catch (e) {
      // error surfaces via parent
    } finally {
      setSaving(false);
    }
  };

  const handleActivate = async () => {
    setActioning(true);
    try {
      await onActivate(strategy.id);
    } finally {
      setActioning(false);
    }
  };

  const handleReasonSubmit = () => {
    if (!reason.trim() || !reasonDialog) return;
    if (reasonDialog === "pause") onPause(strategy.id, reason);
    else onKill(strategy.id, reason);
    setReasonDialog(null);
    setReason("");
  };

  const handleDelete = () => {
    onDelete(strategy.id);
    onBack();
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <button
            onClick={onBack}
            className="p-1.5 rounded-lg hover:bg-muted/20 transition-colors"
          >
            <ArrowLeft size={16} className="text-muted-foreground" />
          </button>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-semibold">{strategy.name}</h2>
              <span
                className={cn(
                  "px-2 py-0.5 rounded-md font-medium text-xs font-medium",
                  statusColors[strategy.status],
                )}
              >
                {strategy.status}
              </span>
            </div>
            <p className="text-xs text-muted-foreground/50">
              {strategy.type.replace(/_/g, " ")} · Created{" "}
              {new Date(strategy.created_at).toLocaleDateString()}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={() => setEditOpen(true)}
            className="p-1.5 rounded-lg hover:bg-muted/20 transition-colors"
            title="Edit"
          >
            <Edit3 size={14} className="text-muted-foreground" />
          </button>
          {(strategy.status === "draft" ||
            strategy.status === "testing" ||
            strategy.status === "paused") && (
            <button
              onClick={handleActivate}
              disabled={actioning}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-success/15 text-success text-xs hover:bg-success/25 transition-colors disabled:opacity-40"
            >
              <Play size={12} /> Activate
            </button>
          )}
          {strategy.status === "active" && (
            <button
              onClick={() => setReasonDialog("pause")}
              disabled={actioning}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-warning/15 text-warning text-xs hover:bg-warning/25 transition-colors disabled:opacity-40"
            >
              <Pause size={12} /> Pause
            </button>
          )}
          {strategy.status !== "killed" && (
            <button
              onClick={() => setReasonDialog("kill")}
              disabled={actioning}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-destructive/15 text-destructive text-xs hover:bg-destructive/25 transition-colors disabled:opacity-40"
            >
              <Skull size={12} /> Kill
            </button>
          )}
        </div>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-4 gap-4">
        <div className="panel p-4 space-y-1">
          <p className="text-xs text-muted-foreground/60">Total Revenue</p>
          <p className="text-xl font-mono font-semibold text-success">
            ${strategy.total_revenue.toFixed(2)}
          </p>
        </div>
        <div className="panel p-4 space-y-1">
          <p className="text-xs text-muted-foreground/60">Expenses</p>
          <p className="text-xl font-mono font-semibold text-destructive">
            ${strategy.total_expenses.toFixed(2)}
          </p>
        </div>
        <div className="panel p-4 space-y-1">
          <p className="text-xs text-muted-foreground/60">Net Income</p>
          <p
            className={cn(
              "text-xl font-mono font-semibold",
              net >= 0 ? "text-success" : "text-destructive",
            )}
          >
            {net >= 0 ? "+" : ""}${net.toFixed(2)}
          </p>
        </div>
        <div className="panel p-4 space-y-1">
          <p className="text-xs text-muted-foreground/60">Health</p>
          <div className="flex items-center gap-2">
            <Circle
              size={8}
              className={cn(
                "fill-current",
                healthColors[strategy.health_status],
              )}
            />
            <p
              className={cn(
                "text-sm font-medium",
                healthColors[strategy.health_status],
              )}
            >
              {strategy.health_status}
            </p>
          </div>
          {strategy.last_health && (
            <p className="text-xs text-muted-foreground/50">
              Last: {new Date(strategy.last_health).toLocaleTimeString()}
            </p>
          )}
        </div>
      </div>

      {/* Config + Decisions */}
      <div className="grid grid-cols-2 gap-4">
        {/* Config */}
        <div className="panel p-4 space-y-3">
          <h3 className="text-xs font-medium text-muted-foreground">Configuration</h3>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-[13px] text-muted-foreground/50">Worker URL</span>
              <a
                href={strategy.config.worker_url}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 text-[13px] text-accent hover:underline"
              >
                {strategy.config.worker_url.replace(/^https?:\/\//, "").slice(0, 40)}
                <ExternalLink size={10} />
              </a>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-[13px] text-muted-foreground/50">Capital Limit</span>
              <span className="text-[13px] font-mono">${strategy.capital_limit.toFixed(2)}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-[13px] text-muted-foreground/50">Daily Loss Cap</span>
              <span className="text-[13px] font-mono">${strategy.daily_loss_cap.toFixed(2)}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-[13px] text-muted-foreground/50">Health Endpoint</span>
              <span className="text-[13px] font-mono text-muted-foreground/60">{strategy.config.health_endpoint}</span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-[13px] text-muted-foreground/50">Revenue Endpoint</span>
              <span className="text-[13px] font-mono text-muted-foreground/60">{strategy.config.revenue_endpoint}</span>
            </div>
          </div>

          {/* Danger Zone */}
          <div className="pt-3 border-t border-border/18">
            {deleteConfirm ? (
              <div className="flex items-center justify-between">
                <span className="text-xs text-destructive">Are you sure?</span>
                <div className="flex items-center gap-2">
                  <button
                    onClick={() => setDeleteConfirm(false)}
                    className="px-2 py-1 rounded text-xs text-muted-foreground hover:bg-muted/20"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={handleDelete}
                    className="px-2 py-1 rounded text-xs text-destructive bg-destructive/10 hover:bg-destructive/20"
                  >
                    Delete
                  </button>
                </div>
              </div>
            ) : (
              <button
                onClick={() => setDeleteConfirm(true)}
                className="flex items-center gap-1.5 text-xs text-muted-foreground/60 hover:text-destructive transition-colors"
              >
                <Trash2 size={12} /> Delete strategy
              </button>
            )}
          </div>
        </div>

        {/* Decisions for this strategy */}
        <div className="panel p-4 space-y-3">
          <h3 className="text-xs font-medium text-muted-foreground">
            Decision History ({decisions.length})
          </h3>
          {decisions.length === 0 ? (
            <p className="text-xs text-muted-foreground/50 py-4 text-center">
              No decisions logged for this strategy
            </p>
          ) : (
            <div className="space-y-2 max-h-[280px] overflow-y-auto pr-1">
              {decisions.map((dec) => (
                <div
                  key={dec.id}
                  className="px-3 py-2 rounded-lg bg-muted/10 border border-border/18 space-y-1"
                >
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-medium">
                      {dec.action.replace(/_/g, " ")}
                    </span>
                    <span className="text-xs text-muted-foreground/50">
                      {new Date(dec.timestamp).toLocaleString()}
                    </span>
                  </div>
                  <p className="text-[13px] text-muted-foreground/60">
                    {dec.reasoning}
                  </p>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Edit Dialog */}
      <Dialog.Root open={editOpen} onOpenChange={setEditOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-[85] bg-black/50 backdrop-blur-sm" />
          <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[86] w-[420px] glass rounded-xl border border-border/45 shadow-2xl p-6 space-y-5">
            <Dialog.Title className="text-sm font-semibold">
              Edit Strategy
            </Dialog.Title>
            <div className="space-y-3">
              <div>
                <label className="text-[13px] text-muted-foreground mb-1 block">Name</label>
                <input
                  type="text"
                  value={editName}
                  onChange={(e) => setEditName(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/45 text-sm focus:outline-none focus:border-accent/40 transition-colors"
                />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">Capital Limit ($)</label>
                  <input
                    type="number"
                    value={editCapital}
                    onChange={(e) => setEditCapital(parseFloat(e.target.value) || 0)}
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/45 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
                  />
                </div>
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">Daily Loss Cap ($)</label>
                  <input
                    type="number"
                    value={editLossCap}
                    onChange={(e) => setEditLossCap(parseFloat(e.target.value) || 0)}
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/45 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
                  />
                </div>
              </div>
            </div>
            <div className="flex items-center justify-end gap-3 pt-2">
              <Dialog.Close asChild>
                <button className="px-3 py-2 rounded-lg text-sm text-muted-foreground hover:bg-muted/20 transition-colors">
                  Cancel
                </button>
              </Dialog.Close>
              <button
                onClick={handleSaveEdit}
                disabled={saving || !editName.trim()}
                className="px-4 py-2 rounded-lg bg-accent/15 text-accent text-sm font-medium hover:bg-accent/25 transition-colors disabled:opacity-40"
              >
                {saving ? "Saving..." : "Save"}
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      {/* Reason Dialog */}
      <Dialog.Root
        open={reasonDialog !== null}
        onOpenChange={(open) => {
          if (!open) {
            setReasonDialog(null);
            setReason("");
          }
        }}
      >
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-[85] bg-black/50 backdrop-blur-sm" />
          <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[86] w-[400px] glass rounded-xl border border-border/45 shadow-2xl p-6 space-y-5">
            <Dialog.Title className="text-sm font-semibold">
              {reasonDialog === "kill" ? "Kill Strategy" : "Pause Strategy"}
            </Dialog.Title>
            <textarea
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder="Why are you taking this action?"
              rows={3}
              className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/45 text-sm placeholder:text-muted-foreground/35 focus:outline-none focus:border-accent/40 transition-colors resize-none"
            />
            <div className="flex items-center justify-end gap-3">
              <Dialog.Close asChild>
                <button className="px-3 py-2 rounded-lg text-sm text-muted-foreground hover:bg-muted/20 transition-colors">
                  Cancel
                </button>
              </Dialog.Close>
              <button
                onClick={handleReasonSubmit}
                disabled={!reason.trim()}
                className={cn(
                  "px-4 py-2 rounded-lg text-sm font-medium transition-colors disabled:opacity-40",
                  reasonDialog === "kill"
                    ? "bg-destructive/15 text-destructive hover:bg-destructive/25"
                    : "bg-warning/15 text-warning hover:bg-warning/25",
                )}
              >
                {reasonDialog === "kill" ? "Kill" : "Pause"}
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  );
}
