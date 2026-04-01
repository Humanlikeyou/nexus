/**
 * OpenClawStatusCard — Tactical Command Panel
 *
 * Displays live OpenClaw gateway health with auto-refresh every 60 seconds.
 * Status: healthy (green) | degraded (yellow) | offline (red)
 */

import { useState, useEffect, useCallback, useRef } from "react";
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

function StatusBadge({ status }: { status: OpenClawHealth["status"] }) {
  const variants = {
    healthy: {
      icon: <Wifi size={10} />,
      label: "Healthy",
      className: "bg-success/12 text-success border border-success/30",
    },
    degraded: {
      icon: <AlertTriangle size={10} />,
      label: "Degraded",
      className: "bg-warning/12 text-warning border border-warning/20",
    },
    offline: {
      icon: <WifiOff size={10} />,
      label: "Offline",
      className: "bg-destructive/12 text-destructive border border-destructive/20",
    },
  } as const;

  const v = variants[status];
  return (
    <div className={cn("badge-status", v.className)}>
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
    <div className="flex items-center gap-2.5">
      <div className="text-muted-foreground/60 flex-shrink-0">{icon}</div>
      <div className="flex-1 min-w-0">
        <p className="metric-label">{label}</p>
        <p className="text-[11px] font-mono text-foreground/85 truncate mt-0.5">{value}</p>
      </div>
      {ok ? (
        <CheckCircle2 size={12} className="text-success/80 flex-shrink-0" />
      ) : (
        <XCircle size={12} className="text-destructive/80 flex-shrink-0" />
      )}
    </div>
  );
}

interface OpenClawStatusCardProps {
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

  const onHealthFetchedRef = useRef(onHealthFetched);
  onHealthFetchedRef.current = onHealthFetched;
  const healthRef = useRef(health);
  healthRef.current = health;
  const prevStatusRef = useRef<OpenClawHealth["status"] | null>(null);
  const lastAutoRestartRef = useRef<number>(0);
  const AUTO_RESTART_COOLDOWN_MS = 5 * 60 * 1000;

  const fetchHealth = useCallback(async (manual = false) => {
    if (manual) setRefreshing(true);
    else if (!healthRef.current) setLoading(true);

    try {
      const result = await getOpenClawHealth();
      setHealth(result);
      setError(null);
      setLastRefresh(new Date());
      onHealthFetchedRef.current?.(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    fetchHealth();
    const interval = setInterval(() => fetchHealth(), REFRESH_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [fetchHealth]);

  useEffect(() => {
    if (!health) return;
    const prev = prevStatusRef.current;
    prevStatusRef.current = health.status;
    if (prev === null) return;
    if (prev === health.status) return;

    if (health.status === "offline") {
      const now = Date.now();
      if (now - lastAutoRestartRef.current > AUTO_RESTART_COOLDOWN_MS) {
        lastAutoRestartRef.current = now;
        restartOpenclaw()
          .then((msg) => {
            setRestartMsg(msg);
            setTimeout(() => fetchHealth(true), 6000);
          })
          .catch((e) => {
            setRestartMsg(`Auto-restart failed: ${e}`);
          });
      } else {
      }
    }
  }, [health, fetchHealth]);

  const handleRestart = useCallback(async () => {
    setRestarting(true);
    setRestartMsg(null);
    try {
      const msg = await restartOpenclaw();
      setRestartMsg(msg);
      setTimeout(() => fetchHealth(true), 4000);
    } catch (e) {
      setRestartMsg(`Failed: ${e}`);
    } finally {
      setRestarting(false);
    }
  }, [fetchHealth]);

  // Skeleton
  if (loading && !health) {
    return (
      <div className="panel p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="heading-section">OpenClaw Gateway</h3>
          <div className="h-5 w-16 bg-muted/15 rounded-sm animate-pulse" />
        </div>
        <div className="space-y-3">
          {Array.from({ length: 4 }).map((_, i) => (
            <div key={i} className="flex items-center gap-2.5">
              <div className="w-3 h-3 bg-muted/15 rounded-sm animate-pulse" />
              <div className="space-y-1 flex-1">
                <div className="h-2 bg-muted/15 rounded-sm animate-pulse w-1/3" />
                <div className="h-3 bg-muted/15 rounded-sm animate-pulse w-1/2" />
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }

  // Error state
  if (error && !health) {
    return (
      <div className="panel p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="heading-section">OpenClaw Gateway</h3>
          <div className="badge-status bg-destructive/12 text-destructive border border-destructive/20">
            <WifiOff size={10} /> Offline
          </div>
        </div>
        <p className="text-[10px] font-mono text-muted-foreground/60">
          Health check failed — gateway may be down.
        </p>
        <button
          onClick={() => fetchHealth(true)}
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-sm bg-muted/10 text-muted-foreground text-[10px] font-mono hover:bg-muted/20 transition-colors"
        >
          <RefreshCw size={10} className={cn(refreshing && "animate-spin")} />
          Retry
        </button>
      </div>
    );
  }

  const h = health!;

  const channelValue = (v: string) =>
    v === "connected" ? "Connected"
    : v === "disconnected" ? "Disconnected"
    : v === "not_linked" ? "Not linked"
    : "Unknown";

  const channelOk = (v: string) => v === "connected";

  return (
    <div className="panel p-4 space-y-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h3 className="heading-section">OpenClaw Gateway</h3>
        <div className="flex items-center gap-2">
          <StatusBadge status={h.status} />
          <button
            onClick={() => fetchHealth(true)}
            className="p-1 rounded-sm text-muted-foreground/50 hover:text-muted-foreground/60 hover:bg-muted/10 transition-colors"
            title="Refresh"
          >
            <RefreshCw size={11} className={cn(refreshing && "animate-spin")} />
          </button>
        </div>
      </div>

      {/* Health rows */}
      <div className="space-y-2.5">
        <HealthRow
          icon={<Tag size={12} />}
          label="Version"
          value={h.version ?? "unknown"}
          ok={!!h.version}
        />
        <HealthRow
          icon={<Server size={12} />}
          label="Gateway (port 18789)"
          value={h.gateway ? "Reachable" : "Unreachable"}
          ok={h.gateway}
        />
        <HealthRow
          icon={<Wifi size={12} />}
          label="RPC Probe"
          value={h.rpc ? "OK" : "No response"}
          ok={h.rpc}
        />
        <HealthRow
          icon={<Brain size={12} />}
          label="Memory Search"
          value={h.memory_search === "configured" ? "Configured" : "Missing"}
          ok={h.memory_search === "configured"}
        />
        <HealthRow
          icon={<MessageCircle size={12} />}
          label="Telegram"
          value={channelValue(h.channels.telegram)}
          ok={channelOk(h.channels.telegram)}
        />
      </div>

      {/* Degraded / offline notice */}
      {h.status !== "healthy" && (
        <div
          className={cn(
            "px-2.5 py-2 rounded-sm text-[10px] font-mono leading-relaxed space-y-1.5",
            h.status === "offline"
              ? "bg-destructive/5 text-destructive/70 border border-destructive/25"
              : "bg-warning/5 text-warning/70 border border-warning/25",
          )}
        >
          {h.status === "offline" && (
            <div className="flex items-start justify-between gap-2">
              <p>
                <strong>Gateway unreachable.</strong> Run{" "}
                <code className="font-mono">openclaw gateway start</code> or restart
                via LaunchAgent.
              </p>
              <button
                onClick={handleRestart}
                disabled={restarting}
                className="btn-kill flex-shrink-0 flex items-center gap-1 !px-2 !py-1 !text-[10px]"
              >
                {restarting ? (
                  <RefreshCw size={10} className="animate-spin" />
                ) : (
                  <RotateCcw size={10} />
                )}
                Restart
              </button>
            </div>
          )}
          {h.status === "degraded" && h.memory_search === "missing" && (
            <p>
              <strong>Memory search not configured.</strong> Install a local model
              or switch to a remote provider.
            </p>
          )}
          {restartMsg && (
            <p className="text-[10px] opacity-60">{restartMsg}</p>
          )}
        </div>
      )}

      {/* Last refresh */}
      {lastRefresh && (
        <div className="flex items-center gap-1 text-[10px] font-mono text-muted-foreground/45">
          <Clock size={9} />
          {lastRefresh.toLocaleTimeString()} / 60s
        </div>
      )}
    </div>
  );
}
