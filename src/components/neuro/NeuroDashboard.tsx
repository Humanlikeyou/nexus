import { BrainWaves } from "./BrainWaves";
import { FrequencyBands } from "./FrequencyBands";
import { FocusScore } from "./FocusScore";
import { SessionHistory } from "./SessionHistory";
import { ProtocolPicker } from "./ProtocolPicker";
import { BrainSnapshotCard } from "./BrainSnapshotCard";
import { ProgressStats } from "./ProgressStats";
import { CalibrationPanel } from "./CalibrationPanel";
import { SignalQualityBar } from "./SignalQualityBar";
import { ArtifactTicker } from "./ArtifactTicker";
import { SessionModePicker } from "./SessionModePicker";
import { ReplayControls } from "./ReplayControls";
import { ElectrodePanel } from "./ElectrodePanel";
import type {
  FrequencyBandPowers,
  NfProtocol,
  RecordedSession,
  BrainSnapshot,
  EegDisplayConfig,
  ArtifactEvent,
  ScoreConfidenceState,
  SessionMode,
  SimpleSessionPhase,
  SimpleSessionSummary,
  ReplayMode,
  ReplayFileInfo,
  ElectrodeState,
  ElectrodeMode,
} from "@/lib/types";
import type { EegDebugInfo } from "@/hooks/useMuse";
import type { EegDisplayData } from "@/hooks/useEegDisplay";
import type { CalibrationStatus, CalibrationBaseline, CalibratedBands } from "@/hooks/useCalibration";
import type { SignalQualityState } from "@/hooks/useSignalQuality";

interface NeuroDashboardProps {
  // BLE Panel is rendered by parent (App.tsx) above this component
  fftBands: FrequencyBandPowers;
  eegData: Record<string, number[]>;
  streaming: boolean;
  museConnected: boolean;
  sessions: RecordedSession[];
  onSelectProtocol: (protocol: NfProtocol, durationMin: number) => void;
  onSnapshotComplete?: (snapshot: BrainSnapshot) => void;
  eegDebug?: EegDebugInfo;
  // Display pipeline
  displayData: EegDisplayData;
  displayConfig: EegDisplayConfig;
  onUpdateDisplayConfig: (patch: Partial<EegDisplayConfig>) => void;
  // Calibration
  calibrationStatus: CalibrationStatus;
  calibrationProgress: number;
  calibrationBaseline: CalibrationBaseline | null;
  calibratedBands: CalibratedBands;
  onStartCalibration: () => void;
  onResetCalibration: () => void;
  // Signal quality
  signalQuality: SignalQualityState;
  // Artifacts
  recentArtifacts: ArtifactEvent[];
  artifactRate: number;
  // Score confidence
  scoreConfidence: ScoreConfidenceState;
  // Simple session
  simpleSessionPhase: SimpleSessionPhase;
  simpleSessionMode: SessionMode | null;
  simpleSessionElapsed: number;
  simpleSessionRemaining: number;
  simpleSessionSummary: SimpleSessionSummary | null;
  onStartSimpleSession: (mode: SessionMode, durationMin: number) => void;
  onEndSimpleSession: () => void;
  onResetSimpleSession: () => void;
  // NF session active (for mutual exclusion)
  nfSessionActive: boolean;
  // Electrode pathway
  electrodeState: ElectrodeState;
  electrodeDerivedMode: ElectrodeMode;
  electrodeOutputAllowed: boolean;
  // Replay controls
  replayMode: ReplayMode;
  replayFiles: ReplayFileInfo[];
  replaySpeed: number;
  replayActiveFilename: string | null;
  replayActiveDemoMode: string | null;
  replayRecordingStartedAt: number | null;
  onStartRecording: () => void;
  onStopRecording: () => void;
  onStartReplay: (filename: string, speed: number) => void;
  onStopReplay: () => void;
  onStartDemo: (mode: string, speed: number) => void;
  onStopDemo: () => void;
  onSetReplaySpeed: (speed: number) => void;
  onRefreshReplayFiles: () => void;
}

export function NeuroDashboard({
  fftBands,
  eegData,
  streaming,
  museConnected,
  sessions,
  onSelectProtocol,
  onSnapshotComplete,
  eegDebug,
  displayData,
  displayConfig,
  onUpdateDisplayConfig,
  calibrationStatus,
  calibrationProgress,
  calibrationBaseline,
  calibratedBands,
  onStartCalibration,
  onResetCalibration,
  signalQuality,
  recentArtifacts,
  artifactRate,
  scoreConfidence,
  simpleSessionPhase,
  simpleSessionMode,
  simpleSessionElapsed,
  simpleSessionRemaining,
  simpleSessionSummary,
  onStartSimpleSession,
  onEndSimpleSession,
  onResetSimpleSession,
  nfSessionActive,
  electrodeState,
  electrodeDerivedMode,
  electrodeOutputAllowed,
  replayMode,
  replayFiles,
  replaySpeed,
  replayActiveFilename,
  replayActiveDemoMode,
  replayRecordingStartedAt,
  onStartRecording,
  onStopRecording,
  onStartReplay,
  onStopReplay,
  onStartDemo,
  onStopDemo,
  onSetReplaySpeed,
  onRefreshReplayFiles,
}: NeuroDashboardProps) {
  const museSessions = sessions.filter((s) => s.device_type === "muse");
  const isCalibrated = calibrationStatus === "done";
  const simpleSessionActive = simpleSessionPhase !== "idle";

  return (
    <div className="space-y-5">
      {/* Replay / Demo / Record controls */}
      <ReplayControls
        mode={replayMode}
        streaming={streaming}
        replayFiles={replayFiles}
        speed={replaySpeed}
        activeFilename={replayActiveFilename}
        activeDemoMode={replayActiveDemoMode}
        recordingStartedAt={replayRecordingStartedAt}
        onStartRecording={onStartRecording}
        onStopRecording={onStopRecording}
        onStartReplay={onStartReplay}
        onStopReplay={onStopReplay}
        onStartDemo={onStartDemo}
        onStopDemo={onStopDemo}
        onSetSpeed={onSetReplaySpeed}
        onRefreshFiles={onRefreshReplayFiles}
      />

      {/* Protocol Picker — hidden when simple session active */}
      {!simpleSessionActive && (
        <ProtocolPicker
          museConnected={museConnected}
          museStreaming={streaming}
          onSelect={onSelectProtocol}
        />
      )}

      {/* Simple Session Picker — hidden when NF session active */}
      {!nfSessionActive && (
        <SessionModePicker
          phase={simpleSessionPhase}
          activeMode={simpleSessionMode}
          elapsed={simpleSessionElapsed}
          remaining={simpleSessionRemaining}
          summary={simpleSessionSummary}
          streaming={streaming}
          onStart={onStartSimpleSession}
          onEnd={onEndSimpleSession}
          onReset={onResetSimpleSession}
        />
      )}

      {/* Calibration */}
      {streaming && (
        <CalibrationPanel
          status={calibrationStatus}
          progress={calibrationProgress}
          baseline={calibrationBaseline}
          streaming={streaming}
          onStart={onStartCalibration}
          onReset={onResetCalibration}
        />
      )}

      {/* Signal Quality */}
      <SignalQualityBar quality={signalQuality} streaming={streaming} />

      {/* Artifact Ticker */}
      <ArtifactTicker
        artifacts={recentArtifacts}
        artifactRate={artifactRate}
        streaming={streaming}
      />

      {/* Electrode pathway */}
      <ElectrodePanel
        state={electrodeState}
        derivedMode={electrodeDerivedMode}
        outputAllowed={electrodeOutputAllowed}
        streaming={streaming}
      />

      {/* Gauges + Snapshot */}
      <div className="grid grid-cols-3 gap-4">
        <FocusScore
          score={scoreConfidence.displayFocus}
          label={isCalibrated ? "Focus (cal)" : "Focus"}
          color="#0891b2"
          confidence={scoreConfidence.confidence}
          frozen={scoreConfidence.frozen}
        />
        <FocusScore
          score={scoreConfidence.displayRelax}
          label={isCalibrated ? "Relaxation (cal)" : "Relaxation"}
          color="#10b981"
          confidence={scoreConfidence.confidence}
          frozen={scoreConfidence.frozen}
        />
        <BrainSnapshotCard
          streaming={streaming}
          eegData={eegData}
          onSnapshotComplete={onSnapshotComplete}
        />
      </div>

      {/* Frequency Bands + BrainWaves */}
      <div className="grid grid-cols-2 gap-4">
        <FrequencyBands
          bands={{
            delta: fftBands.delta || 15,
            theta: fftBands.theta || 22,
            alpha: fftBands.alpha || 35,
            beta: fftBands.beta || 20,
            gamma: fftBands.gamma || 8,
          }}
          smr={fftBands.smr}
        />
        <BrainWaves
          displayData={displayData}
          config={displayConfig}
          onUpdateConfig={onUpdateDisplayConfig}
          streaming={streaming}
          eegDebug={eegDebug}
          channelQuality={signalQuality.channels}
        />
      </div>

      {/* Progress + Session History */}
      <div className="grid grid-cols-2 gap-4">
        <ProgressStats sessions={museSessions} />
        <SessionHistory sessions={museSessions} />
      </div>
    </div>
  );
}
