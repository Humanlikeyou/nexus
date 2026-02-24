import {
  Cpu,
  Radio,
  WifiOff,
  ShieldCheck,
  Battery,
  Zap,
} from "lucide-react";
import type { ElectrodeState, ElectrodeMode } from "@/lib/types";
import { cn } from "@/lib/utils";

interface ElectrodePanelProps {
  state: ElectrodeState;
  derivedMode: ElectrodeMode;
  outputAllowed: boolean;
  streaming: boolean;
}

const modeConfig: Record<ElectrodeMode, { color: string; label: string }> = {
  off: { color: "text-muted-foreground/50", label: "Off" },
  read: { color: "text-accent", label: "Read" },
  output: { color: "text-destructive", label: "Output" },
  unknown: { color: "text-warning", label: "Unknown" },
};

export function ElectrodePanel({
  state,
  derivedMode,
  outputAllowed,
  streaming,
}: ElectrodePanelProps) {
  const mode = modeConfig[derivedMode];

  return (
    <div className="glass p-6 space-y-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Cpu size={14} className="text-muted-foreground" />
          <span className="text-xs text-muted-foreground uppercase tracking-wider">
            Electrode Pathway
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          {/* Mode badge */}
          <div
            className={cn(
              "flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium",
              derivedMode === "read"
                ? "bg-accent/10 text-accent"
                : derivedMode === "off"
                  ? "bg-muted/20 text-muted-foreground"
                  : "bg-warning/10 text-warning",
            )}
          >
            {derivedMode === "read" ? (
              <Radio size={10} />
            ) : (
              <WifiOff size={10} />
            )}
            {mode.label}
          </div>
        </div>
      </div>

      {/* Status row */}
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-1.5">
          <div
            className={cn(
              "w-2 h-2 rounded-full",
              state.connected
                ? state.streaming
                  ? "bg-accent animate-pulse-dot"
                  : "bg-success"
                : "bg-muted-foreground/30",
            )}
          />
          <span className="text-xs text-muted-foreground">
            {state.status}
          </span>
        </div>

        {/* Source badge */}
        {state.source !== "muse" && (
          <span className="text-[13px] px-2 py-0.5 rounded-md font-medium bg-muted/20 text-muted-foreground">
            {state.source}
          </span>
        )}
      </div>

      {/* Telemetry (when connected + streaming) */}
      {state.telemetry && streaming && (
        <div className="flex items-center gap-3 text-xs text-muted-foreground">
          {/* Channels */}
          <div className="flex items-center gap-1">
            <span className="text-muted-foreground/50">CH:</span>
            {state.telemetry.channels.map((ch) => (
              <span
                key={ch}
                className="font-mono text-[13px] text-muted-foreground/70"
              >
                {ch}
              </span>
            ))}
          </div>

          {/* Sample rate */}
          <span className="text-muted-foreground/50">
            {state.telemetry.sampleRate}Hz
          </span>

          {/* Battery */}
          {state.telemetry.batteryPct !== null && (
            <div className="flex items-center gap-0.5">
              <Battery size={10} className="text-muted-foreground/50" />
              <span>{state.telemetry.batteryPct}%</span>
            </div>
          )}
        </div>
      )}

      {/* Safety indicator — output lock */}
      <div
        className={cn(
          "flex items-center gap-2 px-3 py-2 rounded-lg border text-xs",
          outputAllowed
            ? "bg-destructive/5 border-destructive/20 text-destructive"
            : "bg-success/5 border-success/20 text-success",
        )}
      >
        {outputAllowed ? (
          <>
            <Zap size={12} />
            <span>Output enabled — use with caution</span>
          </>
        ) : (
          <>
            <ShieldCheck size={12} />
            <span>Output locked off — read-only mode</span>
          </>
        )}
      </div>

      {/* Error */}
      {state.lastError && (
        <p className="text-xs text-destructive px-1">
          {state.lastError}
        </p>
      )}
    </div>
  );
}
