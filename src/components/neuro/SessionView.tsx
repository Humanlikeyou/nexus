import { useState, useCallback } from "react";
import { Pause, Play, Square, VolumeX, Volume2 } from "lucide-react";
import { BrainStateCircle } from "./BrainStateCircle";
import { StateTimeline } from "./StateTimeline";
import { FrequencyBands } from "./FrequencyBands";
import { SessionReport } from "./SessionReport";
import { VisualFeedback } from "./VisualFeedback";
import { GateStatusIndicator } from "./GateStatusIndicator";
import type {
  NfProtocol,
  BrainState,
  SessionPhase,
  StateTimelineEntry,
  FrequencyBandPowers,
  NfSession,
  FeedbackSignal,
} from "@/lib/types";
import type { VisualFeedbackState } from "@/lib/neurofeedback/actuators";
import type { GateStatus } from "@/hooks/useNeurofeedbackSession";

interface SessionViewProps {
  phase: SessionPhase;
  protocol: NfProtocol | null;
  elapsed: number;
  remaining: number;
  duration: number;
  currentState: BrainState;
  inZone: boolean;
  score: number;
  timeline: StateTimelineEntry[];
  fftBands: FrequencyBandPowers;
  muted: boolean;
  signal: FeedbackSignal;
  visualState: VisualFeedbackState;
  gateStatus: GateStatus;
  confidence: number;
  onPause: () => void;
  onResume: () => void;
  onEnd: () => NfSession | null;
  onReset: () => void;
  onSetMuted: (m: boolean) => void;
  onSaveSession: (session: NfSession) => void;
}

function formatTimer(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
}

export function SessionView({
  phase,
  protocol,
  elapsed,
  remaining,
  duration,
  currentState,
  inZone,
  score,
  timeline,
  fftBands,
  muted,
  signal,
  visualState,
  gateStatus,
  confidence,
  onPause,
  onResume,
  onEnd,
  onReset,
  onSetMuted,
  onSaveSession,
}: SessionViewProps) {
  const [completedSession, setCompletedSession] = useState<NfSession | null>(null);
  const [showReport, setShowReport] = useState(false);

  const handleEnd = useCallback(() => {
    const session = onEnd();
    if (session) {
      setCompletedSession(session);
      setShowReport(true);
    }
  }, [onEnd]);

  const handleCloseReport = useCallback(() => {
    setShowReport(false);
    setCompletedSession(null);
    onReset();
  }, [onReset]);

  const handleSaveAndClose = useCallback((session: NfSession) => {
    onSaveSession(session);
    setShowReport(false);
    setCompletedSession(null);
    onReset();
  }, [onSaveSession, onReset]);

  // Calibrating / countdown overlay
  if (phase === "calibrating") {
    return (
      <div className="flex flex-col items-center justify-center min-h-[400px] space-y-5">
        <div className="w-20 h-20 rounded-full border-2 border-accent/40 flex items-center justify-center animate-pulse">
          <div className="w-12 h-12 rounded-full bg-accent/15" />
        </div>
        <p className="text-sm text-muted-foreground">Calibrating baseline...</p>
        <p className="text-xs text-muted-foreground/50">Stay relaxed for 5 seconds</p>
      </div>
    );
  }

  if (phase === "countdown") {
    return (
      <div className="flex flex-col items-center justify-center min-h-[400px] space-y-5">
        <p className="text-4xl font-bold text-accent animate-pulse">
          Starting...
        </p>
        <p className="text-xs text-muted-foreground/50">Session begins shortly</p>
      </div>
    );
  }

  const rewardLabels = protocol?.rewardBands.map((b) => b.label.toLowerCase().replace(/\s+/g, "")) || [];
  const inhibitLabels = protocol?.inhibitBands.map((b) => b.label.toLowerCase().replace(/\s+/g, "")) || [];

  const timeInZonePct = timeline.length > 0
    ? Math.round((timeline.filter((e) => e.inZone).length / timeline.length) * 100)
    : 0;

  return (
    <>
      <div className="relative space-y-5">
        {/* Visual feedback overlay */}
        <VisualFeedback
          glowColor={visualState.glowColor}
          glowIntensity={visualState.glowIntensity}
          pulseActive={visualState.pulseActive}
          gated={visualState.gated}
        />

        {/* Header */}
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-base font-semibold">{protocol?.name}</h2>
            <p className="text-xs text-muted-foreground/50">
              {protocol?.eyesClosed ? "Eyes Closed" : "Eyes Open"} · {protocol?.audioMode}
            </p>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-2xl font-mono font-bold tabular-nums">
              {formatTimer(remaining)}
            </span>
            <div className="flex gap-2">
              {phase === "running" ? (
                <button
                  onClick={onPause}
                  className="p-2 rounded-lg bg-muted/20 text-muted-foreground hover:bg-muted/30 transition-colors"
                >
                  <Pause size={16} />
                </button>
              ) : phase === "paused" ? (
                <button
                  onClick={onResume}
                  className="p-2 rounded-lg bg-success/15 text-success hover:bg-success/25 transition-colors"
                >
                  <Play size={16} />
                </button>
              ) : null}
              <button
                onClick={handleEnd}
                className="p-2 rounded-lg bg-destructive/15 text-destructive hover:bg-destructive/25 transition-colors"
              >
                <Square size={16} />
              </button>
            </div>
          </div>
        </div>

        {/* Main content */}
        <div className="flex items-start gap-6">
          {/* Brain state circle */}
          <div className="flex-1 flex flex-col items-center py-4 gap-3">
            <BrainStateCircle state={currentState} inZone={inZone} />
            {/* Signal intensity bar */}
            {(phase === "running" || phase === "paused") && (
              <div className="flex items-center gap-2 w-32">
                <div className="flex-1 h-2 rounded-full bg-muted/20 overflow-hidden">
                  <div
                    className="h-full rounded-full transition-all duration-500"
                    style={{
                      width: `${Math.round(signal.intensity * 100)}%`,
                      backgroundColor: signal.action === "reward"
                        ? "#10b981"
                        : signal.action === "inhibit"
                          ? "#f97316"
                          : "#6b7280",
                      opacity: signal.gated ? 0.3 : 1,
                    }}
                  />
                </div>
                <span className="text-[13px] text-muted-foreground/40 tabular-nums w-7 text-right">
                  {Math.round(signal.intensity * 100)}%
                </span>
              </div>
            )}
          </div>

          {/* Stats sidebar */}
          <div className="w-36 space-y-5 py-4">
            <div className="glass p-3 rounded-lg text-center">
              <p className="text-2xl font-bold tabular-nums">{Math.round(score)}</p>
              <p className="text-xs text-muted-foreground/50">SCORE</p>
            </div>
            <div className="glass p-3 rounded-lg text-center">
              <div className="flex items-center justify-center gap-1.5">
                <div
                  className="w-2 h-2 rounded-full"
                  style={{
                    backgroundColor: inZone ? "#10b981" : "#6b7280",
                  }}
                />
                <span className="text-xs font-medium">
                  {inZone ? "In Zone" : "Out"}
                </span>
              </div>
            </div>
            <div className="glass p-3 rounded-lg text-center">
              <p className="text-lg font-bold tabular-nums">{timeInZonePct}%</p>
              <p className="text-xs text-muted-foreground/50">IN ZONE</p>
            </div>
          </div>
        </div>

        {/* Mini frequency bands */}
        <FrequencyBands
          bands={{
            delta: fftBands.delta,
            theta: fftBands.theta,
            alpha: fftBands.alpha,
            beta: fftBands.beta,
            gamma: fftBands.gamma,
          }}
          smr={fftBands.smr}
          rewardBands={rewardLabels}
          inhibitBands={inhibitLabels}
          compact
        />

        {/* State timeline */}
        <div className="glass p-6 rounded-xl space-y-2">
          <p className="text-xs text-muted-foreground/50">State Timeline</p>
          <StateTimeline timeline={timeline} elapsed={elapsed} duration={duration} />
          <div className="flex gap-3 text-xs text-muted-foreground/40">
            <span className="flex items-center gap-1">
              <span className="w-2 h-2 rounded-sm bg-emerald-500" /> Calm
            </span>
            <span className="flex items-center gap-1">
              <span className="w-2 h-2 rounded-sm bg-amber-500" /> Neutral
            </span>
            <span className="flex items-center gap-1">
              <span className="w-2 h-2 rounded-sm bg-orange-500" /> Active
            </span>
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between text-[13px] text-muted-foreground/40">
          <button
            onClick={() => onSetMuted(!muted)}
            className="flex items-center gap-1.5 hover:text-muted-foreground transition-colors"
          >
            {muted ? <VolumeX size={12} /> : <Volume2 size={12} />}
            {muted ? "Unmute" : "Mute"}
          </button>
          <GateStatusIndicator
            allowed={gateStatus.allowed}
            gated={gateStatus.gated}
            reason={gateStatus.reason}
            confidence={confidence}
          />
          <span>256Hz</span>
        </div>
      </div>

      {/* Report overlay */}
      {showReport && completedSession && (
        <SessionReport
          session={completedSession}
          open={showReport}
          onClose={handleCloseReport}
          onSave={handleSaveAndClose}
        />
      )}
    </>
  );
}
