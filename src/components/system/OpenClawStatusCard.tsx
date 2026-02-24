/**
 * OpenClawStatusCard — Nexus Systems Panel component
 *
 * Displays live OpenClaw gateway health with auto-refresh every 60 seconds.
 * Status: healthy (green) | degraded (yellow) | offline (red)
 */

import { useState, useEffect, useCallback } from "react";
import {
  Wifi,
  WifiOff,
  AlertTriangle,
  Server,
  Tag,
  MessageCircle,
  Brain,
  RefreshCw,
  CheckCircle2,
  XCircle,
  Clock,
  RotateCcw,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { getOpenClawHealth, restartOpenclaw } from "@/lib/api";
import type { OpenClawHealth } from "@/lib/types";

const REFRESH_INTERVAL_MS = 60_000;

// ─── Sub-components ───────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: OpenClawHealth["status"] }) {
  const variants = {
    healthy: {
      icon: <Wifi size={12} />,
      label: "Healthy",
      className: "bg-success/15 text-success",
    },
    degraded: {
      icon: <AlertTriangle size={12} />,
      label: "Degraded",
      className: "bg-warning/15 text-warning",
    },
    offline: {
      icon: <WifiOff size={12} />,
      label: "Offline",
      className: "bg-destructive/15 text-destructive",
    },
  } as const;

  const v = variants[status];
  return (
    <div
      className={cn(
        "flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium",
        v.className,
      )}
    >
      {v.icon}
      {v.label}
    </div>
  );
}

function HealthRow({
  icon,
  label,
  value,
  ok,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  ok: boolean;
}) {
  return (
    <div className="flex items-center gap-3">
      <div className="text-accent flex-shrink-0">{icon}</div>
      <div className="flex-1 min-w-0">
        <p className="text-xs text-muted-foreground">{label}</p>
        <p className="text-sm font-medium truncate">{value}</p>
      </div>
      {ok ? (
        <CheckCircle2 size={14} className="text-success flex-shrink-0" />
      ) : (
        <XCircle size={14} className="text-destructive flex-shrink-0" />
      )}
    </div>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

interface OpenClawStatusCardProps {
  /** Called after each successful health fetch — useful for n8n push. */
  onHealthFetched?: (health: OpenClawHealth) => void;
}

export function OpenClawStatusCard({ onHealthFetched }: OpenClawStatusCardProps = {}) {
  const [health, setHealth] = useState<OpenClawHealth | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lastRefresh, setLastRefresh] = useState<Date | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [restartMsg, setRestartMsg] = useState<string | null>(null);

  const fetchHealth = useCallback(async (manual = false) => {
    if (manual) setRefreshing(true);
    else if (!health) setLoading(true);

    try {
      const result = await getOpenClawHealth();
      setHealth(result);
      setError(null);
      setLastRefresh(new Date());
      onHealthFetched?.(result);
    } catch (err) {
      setError(String(err));
      // Keep last known health visible; mark gateway as suspect
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, [health, onHealthFetched]);

  // Initial fetch + 60s polling
  useEffect(() => {
    fetchHealth();
    const interval = setInterval(() => fetchHealth(), REFRESH_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [fetchHealth]);

  // ─── Skeleton ──────────────────────────────────────────────────────────────
  if (loading && !health) {
    return (
      <div className="glass p-6 space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="text-base font-medium text-muted-foreground">OpenClaw Gateway</h3>
          <div className="h-6 w-20 bg-muted/30 rounded-full animate-pulse" />
        </div>
        <div className="space-y-5">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="flex items-center gap-3">
              <div className="w-4 h-4 bg-muted/30 rounded animate-pulse" />
              <div className="space-y-1.5 flex-1">
                <div className="h-3 bg-muted/30 rounded animate-pulse w-1/3" />
                <div className="h-4 bg-muted/30 rounded animate-pulse w-1/2" />
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }

  // ─── Error state (no prior data) ──────────────────────────────────────────
  if (error && !health) {
    return (
      <div className="glass p-6 space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="text-base font-medium text-muted-foreground">OpenClaw Gateway</h3>
          <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium bg-destructive/15 text-destructive">
            <WifiOff size={12} /> Offline
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          Health check failed — gateway may be down.
        </p>
        <button
          onClick={() => fetchHealth(true)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-muted/20 text-muted-foreground text-xs hover:bg-muted/30 transition-colors"
        >
          <RefreshCw size={12} className={cn(refreshing && "animate-spin")} />
          Retry
        </button>
      </div>
    );
  }

  const handleRestart = useCallback(async () => {
    setRestarting(true);
    setRestartMsg(null);
    try {
      const msg = await restartOpenclaw();
      setRestartMsg(msg);
      // Re-check health after a short delay to let the process come back up
      setTimeout(() => fetchHealth(true), 4000);
    } catch (e) {
      setRestartMsg(`Failed: ${e}`);
    } finally {
      setRestarting(false);
    }
  }, [fetchHealth]);

  // ─── Loaded state ─────────────────────────────────────────────────────────
  const h = health!;

  const channelValue = (v: string) =>
    v === "connected" ? "Connected"
    : v === "disconnected" ? "Disconnected"
    : v === "not_linked" ? "Not linked"
    : "Unknown";

  const channelOk = (v: string) => v === "connected";

  return (
    <div className="glass p-6 space-y-5">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Server size={15} className="text-muted-foreground" />
          <h3 className="text-base font-medium text-muted-foreground">OpenClaw Gateway</h3>
        </div>
        <div className="flex items-center gap-2">
          <StatusBadge status={h.status} />
          <button
            onClick={() => fetchHealth(true)}
            className="p-1 rounded-lg text-muted-foreground/50 hover:text-muted-foreground hover:bg-muted/20 transition-colors"
            title="Refresh"
          >
            <RefreshCw size={13} className={cn(refreshing && "animate-spin")} />
          </button>
        </div>
      </div>

      {/* Health rows */}
      <div className="space-y-3.5">
        <HealthRow
          icon={<Tag size={15} />}
          label="Version"
          value={h.version ?? "unknown"}
          ok={!!h.version}
        />

        <HealthRow
          icon={<Server size={15} />}
          label="Gateway (port 18789)"
          value={h.gateway ? "Reachable" : "Unreachable"}
          ok={h.gateway}
        />

        <HealthRow
          icon={<Wifi size={15} />}
          label="RPC Probe"
          value={h.rpc ? "OK" : "No response"}
          ok={h.rpc}
        />

        <HealthRow
          icon={<Brain size={15} />}
          label="Memory Search"
          value={h.memory_search === "configured" ? "Configured" : "Missing — no local model"}
          ok={h.memory_search === "configured"}
        />

        <HealthRow
          icon={<MessageCircle size={15} />}
          label="Telegram"
          value={channelValue(h.channels.telegram)}
          ok={channelOk(h.channels.telegram)}
        />
      </div>

      {/* Degraded / offline notice */}
      {h.status !== "healthy" && (
        <div
          className={cn(
            "px-3 py-2.5 rounded-xl text-xs leading-relaxed space-y-2",
            h.status === "offline"
              ? "bg-destructive/8 text-destructive border border-destructive/20"
              : "bg-warning/8 text-warning border border-warning/20",
          )}
        >
          {h.status === "offline" && (
            <div className="flex items-start justify-between gap-3">
              <p>
                <strong>Gateway unreachable.</strong> Run{" "}
                <code className="font-mono">openclaw gateway start</code> or restart
                via LaunchAgent.
              </p>
              <button
                onClick={handleRestart}
                disabled={restarting}
                className="flex-shrink-0 flex items-center gap-1 px-2 py-1 rounded-lg bg-destructive/20 text-destructive hover:bg-destructive/30 transition-colors text-[11px] font-medium whitespace-nowrap"
              >
                {restarting ? (
                  <RefreshCw size={11} className="animate-spin" />
                ) : (
                  <RotateCcw size={11} />
                )}
                Restart
              </button>
            </div>
          )}
          {h.status === "degraded" && h.memory_search === "missing" && (
            <p>
              <strong>Memory search not configured.</strong> Install a local model
              or switch to a remote provider:{" "}
              <code className="font-mono text-[13px]">
                openclaw config set agents.defaults.memorySearch.provider openai
              </code>
            </p>
          )}
          {restartMsg && (
            <p className="text-[11px] opacity-70 font-mono">{restartMsg}</p>
          )}
        </div>
      )}

      {/* Last refresh */}
      {lastRefresh && (
        <div className="flex items-center gap-1.5 text-[13px] text-muted-foreground/40">
          <Clock size={11} />
          Refreshed {lastRefresh.toLocaleTimeString()} · every 60 s
        </div>
      )}
    </div>
  );
}
