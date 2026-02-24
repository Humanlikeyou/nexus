/**
 * CalibrationPanel: UI for running EEG baseline calibration.
 * Shows progress during capture, baseline values when done,
 * and allows recalibration.
 */

import type { CalibrationStatus, CalibrationBaseline } from "@/hooks/useCalibration";

interface CalibrationPanelProps {
  status: CalibrationStatus;
  progress: number;
  baseline: CalibrationBaseline | null;
  streaming: boolean;
  onStart: () => void;
  onReset: () => void;
}

export function CalibrationPanel({
  status,
  progress,
  baseline,
  streaming,
  onStart,
  onReset,
}: CalibrationPanelProps) {
  if (status === "idle") {
    return (
      <div className="glass p-6 space-y-2">
        <h4 className="text-xs font-medium text-muted-foreground">Calibration</h4>
        <p className="text-xs text-muted-foreground/70">
          Calibrate to set your baseline. Sit still, eyes open, relaxed for 20 seconds.
        </p>
        <button
          onClick={onStart}
          disabled={!streaming}
          className="px-3 py-1.5 rounded-lg text-xs bg-accent/15 text-accent hover:bg-accent/25 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {streaming ? "Start Calibration" : "Start streaming first"}
        </button>
      </div>
    );
  }

  if (status === "calibrating") {
    const pct = Math.round(progress * 100);
    return (
      <div className="glass p-6 space-y-3">
        <div className="flex items-center justify-between">
          <h4 className="text-xs font-medium text-muted-foreground">Calibrating...</h4>
          <span className="text-xs text-accent font-mono">{pct}%</span>
        </div>
        <p className="text-xs text-muted-foreground/70">
          Stay still, eyes open, relaxed. Collecting baseline...
        </p>
        <div className="h-1.5 bg-muted/20 rounded-full overflow-hidden">
          <div
            className="h-full bg-accent rounded-full transition-all duration-300"
            style={{ width: `${pct}%` }}
          />
        </div>
      </div>
    );
  }

  // status === "done"
  return (
    <div className="glass p-6 space-y-2">
      <div className="flex items-center justify-between">
        <h4 className="text-xs font-medium text-success">Calibrated</h4>
        <button
          onClick={onReset}
          className="text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          Recalibrate
        </button>
      </div>
      {baseline && (
        <div className="flex items-center gap-4 text-xs text-muted-foreground font-mono">
          <span>
            Focus baseline: <span className="text-foreground">{Math.round(baseline.focusRatio * 100)}</span>
          </span>
          <span>
            Relax baseline: <span className="text-foreground">{Math.round(baseline.relaxRatio * 100)}</span>
          </span>
        </div>
      )}
      <p className="text-xs text-muted-foreground/60">
        Scores normalized — 50 = your baseline. Above 50 = above average for you.
      </p>
    </div>
  );
}
