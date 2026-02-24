import { useState } from "react";
import { Leaf, Crosshair, Circle, Play, Square, RotateCcw } from "lucide-react";
import type { SessionMode, SimpleSessionPhase, SimpleSessionSummary } from "@/lib/types";
import { SESSION_MODES } from "@/hooks/useSimpleSession";

interface SessionModePickerProps {
  phase: SimpleSessionPhase;
  activeMode: SessionMode | null;
  elapsed: number;
  remaining: number;
  summary: SimpleSessionSummary | null;
  streaming: boolean;
  onStart: (mode: SessionMode, durationMin: number) => void;
  onEnd: () => void;
  onReset: () => void;
}

const ICON_MAP: Record<string, typeof Leaf> = {
  Leaf,
  Crosshair,
  Circle,
};

const DURATION_OPTIONS = [5, 10, 15, 20];

function formatTime(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function SessionModePicker({
  phase,
  activeMode,
  elapsed,
  remaining,
  summary,
  streaming,
  onStart,
  onEnd,
  onReset,
}: SessionModePickerProps) {
  const [selectedMode, setSelectedMode] = useState<SessionMode>(SESSION_MODES[0]);
  const [duration, setDuration] = useState(selectedMode.defaultDurationMin);

  if (phase === "idle") {
    return (
      <div className="glass p-6 space-y-5">
        <h3 className="text-base font-medium text-muted-foreground">Quick Session</h3>
        <div className="grid grid-cols-3 gap-3">
          {SESSION_MODES.map((mode) => {
            const Icon = ICON_MAP[mode.icon] || Circle;
            const isSelected = selectedMode.id === mode.id;
            return (
              <button
                key={mode.id}
                onClick={() => { setSelectedMode(mode); setDuration(mode.defaultDurationMin); }}
                className={`flex flex-col items-center gap-2 p-4 rounded-lg border transition-colors ${
                  isSelected
                    ? "border-accent bg-accent/10 text-accent"
                    : "border-border/15 bg-muted/10 text-muted-foreground hover:bg-muted/20"
                }`}
              >
                <Icon size={20} />
                <span className="text-xs font-medium">{mode.label}</span>
                <span className="text-xs text-muted-foreground/70">{mode.description}</span>
              </button>
            );
          })}
        </div>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <span className="text-xs text-muted-foreground">Duration:</span>
            <select
              value={duration}
              onChange={(e) => setDuration(Number(e.target.value))}
              className="bg-muted/20 border border-border/15 rounded px-2 py-1 text-xs text-foreground"
            >
              {DURATION_OPTIONS.map((d) => (
                <option key={d} value={d}>{d} min</option>
              ))}
            </select>
          </div>
          <button
            onClick={() => onStart(selectedMode, duration)}
            disabled={!streaming}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-accent/15 text-accent text-xs font-medium hover:bg-accent/25 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          >
            <Play size={12} /> Start
          </button>
        </div>
      </div>
    );
  }

  if (phase === "warmup") {
    return (
      <div className="glass p-6 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-base font-medium text-muted-foreground">
            {activeMode?.label} Session
          </h3>
          <span className="text-xs text-muted-foreground/50 uppercase tracking-wider">
            Warmup
          </span>
        </div>
        <div className="text-center py-4">
          <p className="text-sm text-muted-foreground animate-pulse">Settling...</p>
          <p className="text-xs text-muted-foreground/50 mt-1">Establishing baseline</p>
        </div>
      </div>
    );
  }

  if (phase === "running") {
    const total = elapsed + remaining;
    const progress = total > 0 ? (elapsed / total) * 100 : 0;

    return (
      <div className="glass p-6 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-base font-medium text-muted-foreground">
            {activeMode?.label} Session
          </h3>
          <span className="text-xs text-muted-foreground">
            {formatTime(elapsed)} / {formatTime(total)}
          </span>
        </div>
        <div className="w-full h-2 rounded-full bg-muted/20 overflow-hidden">
          <div
            className="h-full rounded-full bg-accent transition-all duration-1000"
            style={{ width: `${progress}%` }}
          />
        </div>
        <div className="flex items-center justify-between">
          <span className="text-xs text-muted-foreground/50">
            {formatTime(remaining)} remaining
          </span>
          <button
            onClick={onEnd}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-destructive/15 text-destructive text-xs hover:bg-destructive/25 transition-colors"
          >
            <Square size={10} /> End
          </button>
        </div>
      </div>
    );
  }

  if (phase === "cooldown") {
    return (
      <div className="glass p-6 space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-base font-medium text-muted-foreground">
            {activeMode?.label} Session
          </h3>
          <span className="text-xs text-muted-foreground/50 uppercase tracking-wider">
            Cooldown
          </span>
        </div>
        <div className="text-center py-4">
          <p className="text-sm text-muted-foreground animate-pulse">Winding down...</p>
        </div>
      </div>
    );
  }

  if (phase === "complete" && summary) {
    return (
      <div className="glass p-6 space-y-5">
        <div className="flex items-center justify-between">
          <h3 className="text-base font-medium text-muted-foreground">
            Session Complete
          </h3>
          <button
            onClick={onReset}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-muted/20 text-muted-foreground text-xs hover:bg-muted/30 transition-colors"
          >
            <RotateCcw size={10} /> Reset
          </button>
        </div>
        <div className="grid grid-cols-4 gap-3">
          <div className="text-center">
            <p className="text-lg font-bold text-foreground">{formatTime(summary.durationSecs)}</p>
            <p className="text-xs text-muted-foreground">Duration</p>
          </div>
          <div className="text-center">
            <p className="text-lg font-bold text-foreground">{summary.avgPrimary}</p>
            <p className="text-xs text-muted-foreground">Avg Primary</p>
          </div>
          <div className="text-center">
            <p className="text-lg font-bold text-cyan-400">{summary.avgFocus}</p>
            <p className="text-xs text-muted-foreground">Avg Focus</p>
          </div>
          <div className="text-center">
            <p className="text-lg font-bold text-emerald-400">{summary.avgRelax}</p>
            <p className="text-xs text-muted-foreground">Avg Relax</p>
          </div>
        </div>
        <div className="text-center">
          <p className="text-xs text-muted-foreground/50">
            Confidence: {Math.round(summary.avgConfidence * 100)}% &middot; {summary.timeline.length} data points
          </p>
        </div>
      </div>
    );
  }

  return null;
}
