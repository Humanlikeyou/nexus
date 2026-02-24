import { useState } from "react";
import {
  Layers,
  Play,
  Pause,
  Skull,
  Plus,
  ChevronRight,
  Circle,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type {
  ArmStrategy,
  ArmStrategyStatus,
  ArmHealthStatus,
  ArmStrategyConfig,
} from "@/lib/types";
import * as Dialog from "@radix-ui/react-dialog";

const statusColors: Record<ArmStrategyStatus, string> = {
  draft: "text-muted-foreground",
  testing: "text-warning",
  active: "text-success",
  paused: "text-orange-400",
  killed: "text-destructive",
};

const statusBg: Record<ArmStrategyStatus, string> = {
  draft: "bg-muted-foreground/15",
  testing: "bg-warning/15",
  active: "bg-success/15",
  paused: "bg-orange-400/15",
  killed: "bg-destructive/15",
};

const healthDot: Record<ArmHealthStatus, string> = {
  healthy: "bg-success",
  degraded: "bg-warning",
  down: "bg-destructive",
  unknown: "bg-muted-foreground/30",
};

interface StrategyListProps {
  strategies: ArmStrategy[];
  onActivate: (id: string) => void;
  onPause: (id: string, reasoning: string) => void;
  onKill: (id: string, reasoning: string) => void;
  onCreate: (
    name: string,
    type: string,
    config: ArmStrategyConfig,
    capitalLimit: number,
    dailyLossCap: number,
  ) => void;
  onSelect?: (strategy: ArmStrategy) => void;
}

interface CreateForm {
  name: string;
  type: string;
  worker_url: string;
  health_endpoint: string;
  revenue_endpoint: string;
  capital_limit: number;
  daily_loss_cap: number;
}

const emptyForm: CreateForm = {
  name: "",
  type: "api_service",
  worker_url: "",
  health_endpoint: "/health",
  revenue_endpoint: "/revenue",
  capital_limit: 10,
  daily_loss_cap: 2,
};

export function StrategyList({
  strategies,
  onActivate,
  onPause,
  onKill,
  onCreate,
  onSelect,
}: StrategyListProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [form, setForm] = useState<CreateForm>(emptyForm);
  const [reasonDialog, setReasonDialog] = useState<{
    action: "pause" | "kill";
    id: string;
  } | null>(null);
  const [reason, setReason] = useState("");

  const handleCreate = () => {
    if (!form.name.trim() || !form.worker_url.trim()) return;
    onCreate(
      form.name,
      form.type,
      {
        worker_url: form.worker_url,
        health_endpoint: form.health_endpoint,
        revenue_endpoint: form.revenue_endpoint,
      },
      form.capital_limit,
      form.daily_loss_cap,
    );
    setForm(emptyForm);
    setDialogOpen(false);
  };

  const handleReasonSubmit = () => {
    if (!reasonDialog || !reason.trim()) return;
    if (reasonDialog.action === "pause") {
      onPause(reasonDialog.id, reason);
    } else {
      onKill(reasonDialog.id, reason);
    }
    setReasonDialog(null);
    setReason("");
  };

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Layers size={16} className="text-accent" />
          <h3 className="text-base font-medium text-muted-foreground">
            Strategies
          </h3>
          <span className="text-xs text-muted-foreground/40">
            {strategies.filter((s) => s.status === "active").length} active
          </span>
        </div>
        <button
          onClick={() => {
            setForm(emptyForm);
            setDialogOpen(true);
          }}
          className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg bg-accent/15 text-accent text-xs hover:bg-accent/25 transition-colors"
        >
          <Plus size={12} /> Add
        </button>
      </div>

      {strategies.length === 0 ? (
        <div className="py-8 text-center">
          <Layers size={24} className="text-muted-foreground/20 mx-auto mb-2" />
          <p className="text-xs text-muted-foreground/40">
            No strategies yet. Deploy your first revenue worker.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {strategies.map((strat) => {
            const net = strat.total_revenue - strat.total_expenses;
            return (
              <div
                key={strat.id}
                className="group px-4 py-3 rounded-lg bg-muted/15 border border-border/15 space-y-2 cursor-pointer hover:bg-muted/25 transition-colors"
                onClick={() => onSelect?.(strat)}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <div
                      className={cn(
                        "w-2 h-2 rounded-full",
                        healthDot[strat.health_status],
                        strat.status === "active" && strat.health_status === "healthy" && "animate-pulse-dot",
                      )}
                    />
                    <span className="text-sm font-medium">{strat.name}</span>
                    <span
                      className={cn(
                        "px-2 py-0.5 rounded-md font-medium text-xs font-medium",
                        statusBg[strat.status],
                        statusColors[strat.status],
                      )}
                    >
                      {strat.status}
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <span
                      className={cn(
                        "text-xs font-mono",
                        net >= 0 ? "text-success" : "text-destructive",
                      )}
                    >
                      {net >= 0 ? "+" : ""}${net.toFixed(2)}
                    </span>
                    <ChevronRight
                      size={14}
                      className="text-muted-foreground/30 group-hover:text-muted-foreground/60 transition-colors"
                    />
                  </div>
                </div>

                <div className="flex items-center justify-between">
                  <span className="text-xs text-muted-foreground/50">
                    {strat.type.replace(/_/g, " ")} · Cap ${strat.capital_limit}
                  </span>
                  <div
                    className="flex items-center gap-1"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {(strat.status === "draft" || strat.status === "testing" || strat.status === "paused") && (
                      <button
                        onClick={() => onActivate(strat.id)}
                        className="p-1 rounded hover:bg-success/15 transition-colors"
                        title="Activate"
                      >
                        <Play size={12} className="text-success" />
                      </button>
                    )}
                    {strat.status === "active" && (
                      <button
                        onClick={() =>
                          setReasonDialog({ action: "pause", id: strat.id })
                        }
                        className="p-1 rounded hover:bg-warning/15 transition-colors"
                        title="Pause"
                      >
                        <Pause size={12} className="text-warning" />
                      </button>
                    )}
                    {strat.status !== "killed" && (
                      <button
                        onClick={() =>
                          setReasonDialog({ action: "kill", id: strat.id })
                        }
                        className="p-1 rounded hover:bg-destructive/15 transition-colors"
                        title="Kill"
                      >
                        <Skull size={12} className="text-destructive" />
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Create Strategy Dialog */}
      <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-[85] bg-black/50 backdrop-blur-sm" />
          <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[86] w-[460px] glass rounded-xl border border-border/30 shadow-2xl p-6 space-y-5">
            <Dialog.Title className="text-sm font-semibold">
              Deploy Strategy
            </Dialog.Title>

            <div className="space-y-3">
              <div>
                <label className="text-[13px] text-muted-foreground mb-1 block">
                  Name
                </label>
                <input
                  type="text"
                  value={form.name}
                  onChange={(e) =>
                    setForm((f) => ({ ...f, name: e.target.value }))
                  }
                  placeholder="My API Microservice"
                  className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
                />
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">
                    Type
                  </label>
                  <select
                    value={form.type}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, type: e.target.value }))
                    }
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm focus:outline-none focus:border-accent/40 transition-colors"
                  >
                    <option value="api_service">API Service</option>
                    <option value="content_syndication">
                      Content Syndication
                    </option>
                    <option value="data_packaging">Data Packaging</option>
                    <option value="saas_micro">SaaS Micro</option>
                    <option value="affiliate">Affiliate</option>
                  </select>
                </div>
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">
                    Worker URL
                  </label>
                  <input
                    type="text"
                    value={form.worker_url}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, worker_url: e.target.value }))
                    }
                    placeholder="https://my-worker.workers.dev"
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors"
                  />
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">
                    Capital Limit ($)
                  </label>
                  <input
                    type="number"
                    value={form.capital_limit}
                    onChange={(e) =>
                      setForm((f) => ({
                        ...f,
                        capital_limit: parseFloat(e.target.value) || 0,
                      }))
                    }
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
                  />
                </div>
                <div>
                  <label className="text-[13px] text-muted-foreground mb-1 block">
                    Daily Loss Cap ($)
                  </label>
                  <input
                    type="number"
                    value={form.daily_loss_cap}
                    onChange={(e) =>
                      setForm((f) => ({
                        ...f,
                        daily_loss_cap: parseFloat(e.target.value) || 0,
                      }))
                    }
                    className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm font-mono focus:outline-none focus:border-accent/40 transition-colors"
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
                onClick={handleCreate}
                disabled={!form.name.trim() || !form.worker_url.trim()}
                className="px-4 py-2 rounded-lg bg-accent/15 text-accent text-sm font-medium hover:bg-accent/25 transition-colors disabled:opacity-40"
              >
                Deploy
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>

      {/* Reason Dialog (Pause/Kill) */}
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
          <Dialog.Content className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[86] w-[400px] glass rounded-xl border border-border/30 shadow-2xl p-6 space-y-5">
            <Dialog.Title className="text-sm font-semibold">
              {reasonDialog?.action === "kill" ? "Kill Strategy" : "Pause Strategy"}
            </Dialog.Title>
            <div>
              <label className="text-[13px] text-muted-foreground mb-1 block">
                Reasoning (required for decision journal)
              </label>
              <textarea
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                placeholder="Why are you taking this action?"
                rows={3}
                className="w-full px-3 py-2 rounded-lg bg-muted/15 border border-border/30 text-sm placeholder:text-muted-foreground/20 focus:outline-none focus:border-accent/40 transition-colors resize-none"
              />
            </div>
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
                  reasonDialog?.action === "kill"
                    ? "bg-destructive/15 text-destructive hover:bg-destructive/25"
                    : "bg-warning/15 text-warning hover:bg-warning/25",
                )}
              >
                {reasonDialog?.action === "kill" ? "Kill" : "Pause"}
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  );
}
