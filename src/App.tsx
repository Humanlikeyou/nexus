import { useState, useEffect, useCallback, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { listen } from "@tauri-apps/api/event";
import { Shell } from "./components/layout/Shell";
import { useOpenClaw } from "./hooks/useOpenClaw";
import { useMuse } from "./hooks/useMuse";
import { useWhoop } from "./hooks/useWhoop";
import { useFFT } from "./hooks/useFFT";
import { useEegDisplay } from "./hooks/useEegDisplay";
import { useCalibration } from "./hooks/useCalibration";
import { useSignalQuality } from "./hooks/useSignalQuality";
import { useArtifactDetection } from "./hooks/useArtifactDetection";
import { useScoreConfidence } from "./hooks/useScoreConfidence";
import { useSimpleSession } from "./hooks/useSimpleSession";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useNexus } from "./context/NexusContext";
import { readTasks, readContentPipeline, saveSession, listSessions, saveSnapshot, insertEvents } from "./lib/api";
import { sessionToEvents, nfSessionToEvents, snapshotToEvents } from "./lib/events";
import type { PageId, TaskItem, ContentPipelineItem, RecordedSession, NfSession, BrainSnapshot, ReplayMode, DataSource } from "./lib/types";

// Toast + Command Palette + Onboarding
import { ToastContainer } from "./components/ui/Toast";
import { CommandPalette } from "./components/ui/CommandPalette";
import { OnboardingModal } from "./components/ui/OnboardingModal";

// Terrance page components
import { StatusCard } from "./components/terrance/StatusCard";
import { CronSchedule } from "./components/terrance/CronSchedule";
import { TokenUsage } from "./components/terrance/TokenUsage";
import { SkillsList } from "./components/terrance/SkillsList";

// Business page components
import { TaskBoard } from "./components/business/TaskBoard";
import { ContentPipeline } from "./components/business/ContentPipeline";
import { RevenueCard } from "./components/business/RevenueCard";

// Assistant page components
import { CalendarView } from "./components/assistant/CalendarView";
import { EmailSummary } from "./components/assistant/EmailSummary";
import { DailyLog } from "./components/assistant/DailyLog";

// Neuro page components
import { NeuroDashboard } from "./components/neuro/NeuroDashboard";
import { SessionView } from "./components/neuro/SessionView";
import { useAudioFeedback } from "./hooks/useAudioFeedback";
import { useNeurofeedbackSession } from "./hooks/useNeurofeedbackSession";
import { useReplay } from "./hooks/useReplay";
import { useElectrode } from "./hooks/useElectrode";

// Fitness page components
import { RecoveryScore } from "./components/fitness/RecoveryScore";
import { SleepChart } from "./components/fitness/SleepChart";
import { StrainChart } from "./components/fitness/StrainChart";
import { HrvTrend } from "./components/fitness/HrvTrend";
import { useWhoopAnalytics } from "./hooks/useWhoopAnalytics";

// Trajectory page components
import { GoalTracker } from "./components/trajectory/GoalTracker";
import { WellnessScore } from "./components/trajectory/WellnessScore";

// Telegram + Gateway
import { TelegramChat } from "./components/terrance/TelegramChat";
import { GatewayStatus } from "./components/terrance/GatewayStatus";

// OpenClaw health monitoring
import { OpenClawStatusCard } from "./components/system/OpenClawStatusCard";
import { pushHealthToN8n } from "./terrance/system/openclawHealth";

// ARM / Revenue
import { ArmDashboard } from "./components/arm/ArmDashboard";
import { useArm } from "./hooks/useArm";

// Settings
import { SettingsPage } from "./components/settings/SettingsPage";

// Proof Panel
import { ProofPanel } from "./components/ui/ProofPanel";

// BLE connection panel
import {
  Bluetooth,
  BluetoothSearching,
  BluetoothOff,
  Loader2,
  Play,
  Square,
  Circle,
} from "lucide-react";
import { cn } from "./lib/utils";

function BlePanel({
  title,
  devices,
  connectionState,
  scanning,
  connecting,
  error,
  onScan,
  onConnect,
  onDisconnect,
  onStartStream,
  onStopStream,
  streaming,
  recording,
  onToggleRecord,
  replayMode,
  dataSource,
}: {
  title: string;
  devices: Array<{ name: string; address: string; rssi: number | null }>;
  connectionState: { connected: boolean; device_name: string | null; streaming: boolean };
  scanning: boolean;
  connecting?: boolean;
  error: string | null;
  onScan: () => void;
  onConnect: (address: string) => void;
  onDisconnect: () => void;
  onStartStream: () => void;
  onStopStream: () => void;
  streaming: boolean;
  recording?: boolean;
  onToggleRecord?: () => void;
  replayMode?: ReplayMode;
  dataSource?: DataSource;
}) {
  const isVirtual = replayMode === "replaying" || replayMode === "demo";

  // During replay/demo, show a simplified panel with badge
  if (isVirtual) {
    const badgeColor = replayMode === "demo" ? "blue" : "yellow";
    const badgeLabel = replayMode === "demo" ? "Demo Mode" : "Replay Mode";
    return (
      <div className="glass p-6 space-y-5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Bluetooth size={16} className="text-accent" />
            <h3 className="text-sm font-medium text-muted-foreground">{title}</h3>
          </div>
          <div className={cn(
            "px-2.5 py-1 rounded-lg text-xs font-semibold uppercase tracking-wide",
            badgeColor === "blue" ? "bg-blue-500/15 text-blue-400" : "bg-yellow-500/15 text-yellow-400",
          )}>
            {badgeLabel}
          </div>
        </div>
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-accent/5 border border-accent/20">
          <div className="w-2 h-2 rounded-full bg-accent animate-pulse-dot" />
          <span className="text-xs text-accent">
            Streaming ({dataSource || replayMode})
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="glass p-6 space-y-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          {connectionState.connected ? (
            <Bluetooth size={16} className="text-accent" />
          ) : connecting ? (
            <Loader2 size={16} className="text-accent animate-spin" />
          ) : scanning ? (
            <BluetoothSearching size={16} className="text-warning animate-pulse" />
          ) : (
            <BluetoothOff size={16} className="text-muted-foreground/50" />
          )}
          <h3 className="text-sm font-medium text-muted-foreground">{title}</h3>
        </div>

        {connectionState.connected ? (
          <div className="flex items-center gap-2">
            {onToggleRecord && streaming && (
              <button
                onClick={onToggleRecord}
                className={cn(
                  "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs transition-colors",
                  recording
                    ? "bg-red-500/15 text-red-400 hover:bg-red-500/25"
                    : "bg-muted/20 text-muted-foreground hover:bg-muted/30",
                )}
              >
                <Circle
                  size={8}
                  className={cn(recording && "fill-red-400 animate-pulse")}
                />
                {recording ? "Recording" : "Record"}
              </button>
            )}
            {streaming ? (
              <button
                onClick={onStopStream}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-destructive/15 text-destructive text-xs hover:bg-destructive/25 transition-colors"
              >
                <Square size={10} /> Stop
              </button>
            ) : (
              <button
                onClick={onStartStream}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-success/15 text-success text-xs hover:bg-success/25 transition-colors"
              >
                <Play size={10} /> Stream
              </button>
            )}
            <button
              onClick={onDisconnect}
              className="px-3 py-1.5 rounded-lg bg-muted/30 text-muted-foreground text-xs hover:bg-muted/50 transition-colors"
            >
              Disconnect
            </button>
          </div>
        ) : (
          <button
            onClick={onScan}
            disabled={scanning}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs transition-colors",
              scanning
                ? "bg-muted/20 text-muted-foreground cursor-wait"
                : "bg-accent/15 text-accent hover:bg-accent/25",
            )}
          >
            {scanning && <Loader2 size={12} className="animate-spin" />}
            {scanning ? "Scanning..." : "Scan"}
          </button>
        )}
      </div>

      {connectionState.connected && (
        <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-success/5 border border-success/20">
          <div className="w-2 h-2 rounded-full bg-success animate-pulse-dot" />
          <span className="text-xs text-success">
            Connected to {connectionState.device_name || "device"}
          </span>
        </div>
      )}

      {!connectionState.connected && devices.length > 0 && (
        <div className="space-y-1.5">
          {devices.map((device) => (
            <button
              key={device.address}
              onClick={() => onConnect(device.address)}
              disabled={connecting || scanning}
              className={cn(
                "w-full flex items-center justify-between px-3 py-2.5 rounded-lg border border-border/15 transition-colors",
                connecting || scanning
                  ? "bg-muted/10 text-muted-foreground/50 cursor-wait"
                  : "bg-muted/15 hover:bg-muted/25",
              )}
            >
              <div className="flex items-center gap-2">
                {connecting ? (
                  <Loader2 size={12} className="text-accent animate-spin" />
                ) : (
                  <Bluetooth size={12} className="text-accent" />
                )}
                <span className="text-xs">
                  {connecting ? "Connecting..." : device.name}
                </span>
              </div>
              {!connecting && device.rssi != null && (
                <span className="text-xs text-muted-foreground/50">
                  {device.rssi} dBm
                </span>
              )}
            </button>
          ))}
        </div>
      )}

      {error && (
        <p className="text-xs text-destructive px-1">{error}</p>
      )}
    </div>
  );
}

const pageTransition = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
  transition: { duration: 0.2, ease: "easeOut" },
};

export default function App() {
  const { setMuseState, setWhoopState, toast, addNotification } = useNexus();

  const openclaw = useOpenClaw();

  // Business data
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [pipeline, setPipeline] = useState<ContentPipelineItem[]>([]);
  const [bizLoading, setBizLoading] = useState(true);

  // Muse
  const muse = useMuse();

  // Replay / Demo
  const replay = useReplay();

  // SMOKE TEST: top-level listener bypassing hook — proves events cross from Rust
  useEffect(() => {
    let count = 0;
    const p = listen("muse-eeg", (e: unknown) => {
      count++;
      if (count <= 3 || count % 200 === 0) {
        console.log(`[APP SMOKE] muse-eeg #${count}`, e);
      }
    });
    return () => { p.then((fn) => fn()); };
  }, []);

  // Whoop
  const whoop = useWhoop();
  const whoopAnalytics = useWhoopAnalytics(
    whoop.hrData,
    whoop.rrIntervals,
    whoop.connectionState.streaming,
  );

  // FFT — computed from live EEG
  const fftBands = useFFT(muse.eegData);

  // EEG display pipeline — reads ring buffer at controlled FPS with decimation
  const { displayData, config: displayConfig, updateConfig: updateDisplayConfig } = useEegDisplay(
    muse.ringBuffer,
    muse.connectionState.streaming,
  );

  // Calibration — baseline normalization for focus/relax scores
  const calibration = useCalibration(fftBands, muse.connectionState.streaming);

  // Signal quality — per-channel electrode contact monitoring
  const signalQuality = useSignalQuality(muse.ringBuffer, muse.connectionState.streaming);

  // Artifact detection — blinks, motion, jaw clench
  const artifacts = useArtifactDetection(muse.ringBuffer, muse.connectionState.streaming);

  // Score confidence + hysteresis — smooth display scores
  const isCalibrated = calibration.status === "done";
  const rawFocus = isCalibrated ? calibration.calibratedBands.calibratedFocus : (fftBands.focusScore || 62);
  const rawRelax = isCalibrated ? calibration.calibratedBands.calibratedRelax : (fftBands.relaxationScore || 71);
  const scoreConfidence = useScoreConfidence(
    rawFocus,
    rawRelax,
    signalQuality.overall,
    signalQuality.frozen,
    artifacts.artifactRate,
  );

  // Simple session modes — Calm/Focus/Neutral
  const simpleSession = useSimpleSession(fftBands, scoreConfidence.confidence, signalQuality.overall);

  // Neurofeedback session engine
  const audioFeedback = useAudioFeedback();
  const nfSession = useNeurofeedbackSession(
    fftBands,
    audioFeedback,
    scoreConfidence.confidence,
    signalQuality.overall,
    artifacts.artifactRate,
    signalQuality.frozen,
  );

  // Electrode pathway — safety-gated device state
  const electrode = useElectrode(
    muse.connectionState.connected,
    muse.connectionState.streaming,
    muse.dataSource,
    scoreConfidence.confidence,
  );

  // ARM — Autonomous Revenue Module
  const arm = useArm();

  // Keyboard shortcuts
  useKeyboardShortcuts();

  // Session recording — Muse
  const [museRecording, setMuseRecording] = useState(false);
  const museRecordStart = useRef<string | null>(null);
  const museRecordBuffer = useRef<Array<{ timestamp: number; channel: string; value: number }>>([]);

  // Session recording — Whoop
  const [whoopRecording, setWhoopRecording] = useState(false);
  const whoopRecordStart = useRef<string | null>(null);
  const whoopRecordBuffer = useRef<Array<{ timestamp: number; heart_rate: number; rr_interval: number | null }>>([]);

  // Session list
  const [sessions, setSessions] = useState<RecordedSession[]>([]);

  // Sync BLE states to context for sidebar dots
  useEffect(() => {
    setMuseState(muse.connectionState);
  }, [muse.connectionState, setMuseState]);

  useEffect(() => {
    setWhoopState(whoop.connectionState);
  }, [whoop.connectionState, setWhoopState]);

  // Notify on BLE connect/disconnect
  const prevMuseConnected = useRef(muse.connectionState.connected);
  useEffect(() => {
    if (muse.connectionState.connected && !prevMuseConnected.current) {
      toast({ type: "success", title: "Muse Connected", description: muse.connectionState.device_name || "Headband" });
      addNotification({ type: "success", title: "Muse Connected", message: `Connected to ${muse.connectionState.device_name || "headband"}`, source: "BLE" });
    } else if (!muse.connectionState.connected && prevMuseConnected.current) {
      toast({ type: "info", title: "Muse Disconnected" });
      addNotification({ type: "info", title: "Muse Disconnected", message: "Headband disconnected", source: "BLE" });
    }
    prevMuseConnected.current = muse.connectionState.connected;
  }, [muse.connectionState.connected, muse.connectionState.device_name, toast, addNotification]);

  const prevWhoopConnected = useRef(whoop.connectionState.connected);
  useEffect(() => {
    if (whoop.connectionState.connected && !prevWhoopConnected.current) {
      toast({ type: "success", title: "Whoop Connected", description: whoop.connectionState.device_name || "Band" });
      addNotification({ type: "success", title: "Whoop Connected", message: `Connected to ${whoop.connectionState.device_name || "band"}`, source: "BLE" });
    } else if (!whoop.connectionState.connected && prevWhoopConnected.current) {
      toast({ type: "info", title: "Whoop Disconnected" });
      addNotification({ type: "info", title: "Whoop Disconnected", message: "Band disconnected", source: "BLE" });
    }
    prevWhoopConnected.current = whoop.connectionState.connected;
  }, [whoop.connectionState.connected, whoop.connectionState.device_name, toast, addNotification]);

  // Buffer recording data
  useEffect(() => {
    if (!museRecording) return;
    const data = muse.eegData;
    for (const [channel, samples] of Object.entries(data)) {
      const last10 = samples.slice(-10);
      for (const value of last10) {
        museRecordBuffer.current.push({ timestamp: Date.now(), channel, value });
      }
    }
  }, [muse.eegData, museRecording]);

  useEffect(() => {
    if (!whoopRecording) return;
    const latest = whoop.hrData.slice(-1)[0];
    if (latest) {
      whoopRecordBuffer.current.push({
        timestamp: latest.timestamp,
        heart_rate: latest.heart_rate,
        rr_interval: latest.rr_interval,
      });
    }
  }, [whoop.hrData, whoopRecording]);

  const toggleMuseRecording = useCallback(async () => {
    if (!museRecording) {
      museRecordStart.current = new Date().toISOString();
      museRecordBuffer.current = [];
      setMuseRecording(true);
      toast({ type: "info", title: "Recording Started", description: "Muse EEG session recording" });
    } else {
      setMuseRecording(false);
      const end = new Date().toISOString();
      const start = museRecordStart.current || end;
      const durationMs = new Date(end).getTime() - new Date(start).getTime();
      const session: RecordedSession = {
        id: crypto.randomUUID(),
        device_type: "muse",
        start,
        end,
        duration_secs: Math.round(durationMs / 1000),
        data_points: museRecordBuffer.current.length,
        file_path: "",
        summary: {
          avg_focus: fftBands.focusScore,
          avg_relaxation: fftBands.relaxationScore,
        },
      };
      try {
        const path = await saveSession(session, museRecordBuffer.current);
        session.file_path = path;
        insertEvents(sessionToEvents(session, path)).catch(() => {});
        toast({ type: "success", title: "Session Saved", description: `${session.duration_secs}s, ${session.data_points} points` });
        addNotification({ type: "success", title: "Muse Session Saved", message: `Recorded ${Math.round(session.duration_secs / 60)}min session`, source: "Recording" });
        loadSessions();
      } catch (e) {
        toast({ type: "error", title: "Save Failed", description: String(e) });
      }
    }
  }, [museRecording, fftBands, toast, addNotification]);

  const toggleWhoopRecording = useCallback(async () => {
    if (!whoopRecording) {
      whoopRecordStart.current = new Date().toISOString();
      whoopRecordBuffer.current = [];
      setWhoopRecording(true);
      toast({ type: "info", title: "Recording Started", description: "Whoop HR session recording" });
    } else {
      setWhoopRecording(false);
      const end = new Date().toISOString();
      const start = whoopRecordStart.current || end;
      const durationMs = new Date(end).getTime() - new Date(start).getTime();
      const buffer = whoopRecordBuffer.current;
      const avgHR = buffer.length > 0
        ? Math.round(buffer.reduce((s, d) => s + d.heart_rate, 0) / buffer.length)
        : 0;
      const maxHR = buffer.length > 0
        ? Math.max(...buffer.map((d) => d.heart_rate))
        : 0;
      const session: RecordedSession = {
        id: crypto.randomUUID(),
        device_type: "whoop",
        start,
        end,
        duration_secs: Math.round(durationMs / 1000),
        data_points: buffer.length,
        file_path: "",
        summary: { avg_hr: avgHR, max_hr: maxHR },
      };
      try {
        const path = await saveSession(session, buffer);
        session.file_path = path;
        insertEvents(sessionToEvents(session, path)).catch(() => {});
        toast({ type: "success", title: "Session Saved", description: `${session.duration_secs}s, avg ${avgHR} BPM` });
        addNotification({ type: "success", title: "Whoop Session Saved", message: `Recorded ${Math.round(session.duration_secs / 60)}min session`, source: "Recording" });
        loadSessions();
      } catch (e) {
        toast({ type: "error", title: "Save Failed", description: String(e) });
      }
    }
  }, [whoopRecording, toast, addNotification]);

  const loadSessions = useCallback(() => {
    listSessions().then(setSessions).catch(() => {});
  }, []);

  // Load business data + sessions on mount
  useEffect(() => {
    Promise.all([readTasks(), readContentPipeline()])
      .then(([t, p]) => {
        setTasks(t);
        setPipeline(p);
      })
      .catch(() => {})
      .finally(() => setBizLoading(false));

    loadSessions();
  }, [loadSessions]);

  // Listen for nexus:refresh custom event
  useEffect(() => {
    const handler = () => openclaw.refresh();
    window.addEventListener("nexus:refresh", handler);
    return () => window.removeEventListener("nexus:refresh", handler);
  }, [openclaw.refresh]);

  const renderPage = (page: PageId) => {
    switch (page) {
      case "terrance":
        return (
          <motion.div key="terrance" {...pageTransition} className="space-y-7">
            <div className="grid grid-cols-3 gap-5">
              <StatusCard status={openclaw.status} loading={openclaw.loading} />
              <TokenUsage loading={openclaw.loading} />
              <SkillsList skills={openclaw.skills} loading={openclaw.loading} />
            </div>
            <div className="grid grid-cols-2 gap-5">
              <GatewayStatus />
              <OpenClawStatusCard onHealthFetched={pushHealthToN8n} />
            </div>
            <div className="grid grid-cols-2 gap-5">
              <CronSchedule jobs={openclaw.cronJobs} loading={openclaw.loading} />
              <TelegramChat />
            </div>
          </motion.div>
        );

      case "business":
        return (
          <motion.div key="business" {...pageTransition} className="space-y-7">
            <TaskBoard tasks={tasks} loading={bizLoading} />
            <div className="grid grid-cols-2 gap-5">
              <ContentPipeline items={pipeline} loading={bizLoading} />
              <RevenueCard />
            </div>
          </motion.div>
        );

      case "assistant":
        return (
          <motion.div key="assistant" {...pageTransition} className="space-y-7">
            <div className="grid grid-cols-2 gap-5">
              <CalendarView placeholder />
              <EmailSummary placeholder />
            </div>
            <DailyLog />
          </motion.div>
        );

      case "neuro":
        if (nfSession.phase !== "idle") {
          return (
            <motion.div key="neuro-session" {...pageTransition} className="space-y-7">
              <SessionView
                phase={nfSession.phase}
                protocol={nfSession.protocol}
                elapsed={nfSession.elapsed}
                remaining={nfSession.remaining}
                duration={nfSession.duration}
                currentState={nfSession.currentState}
                inZone={nfSession.inZone}
                score={nfSession.score}
                timeline={nfSession.timeline}
                fftBands={fftBands}
                muted={audioFeedback.muted}
                signal={nfSession.signal}
                visualState={nfSession.visualState}
                gateStatus={nfSession.gateStatus}
                confidence={scoreConfidence.confidence}
                onPause={nfSession.pauseSession}
                onResume={nfSession.resumeSession}
                onEnd={nfSession.endSession}
                onReset={nfSession.reset}
                onSetMuted={audioFeedback.setMuted}
                onSaveSession={async (session: NfSession) => {
                  try {
                    const path = await saveSession(session, session.state_timeline);
                    insertEvents(nfSessionToEvents(session, path)).catch(() => {});
                    toast({ type: "success", title: "Session Saved", description: `Score: ${session.total_score}, In Zone: ${session.time_in_zone_pct}%` });
                    addNotification({ type: "success", title: "NF Session Saved", message: `${session.protocol_name} — Score: ${session.total_score}`, source: "Neurofeedback" });
                    loadSessions();
                  } catch (e) {
                    toast({ type: "error", title: "Save Failed", description: String(e) });
                  }
                }}
              />
            </motion.div>
          );
        }
        return (
          <motion.div key="neuro" {...pageTransition} className="space-y-7">
            <BlePanel
              title="Muse Headband"
              devices={muse.devices}
              connectionState={muse.connectionState}
              scanning={muse.scanning}
              connecting={muse.connecting}
              error={muse.error}
              onScan={muse.scan}
              onConnect={muse.connect}
              onDisconnect={muse.disconnect}
              onStartStream={muse.startStream}
              onStopStream={muse.stopStream}
              streaming={muse.connectionState.streaming}
              recording={museRecording}
              onToggleRecord={toggleMuseRecording}
              replayMode={replay.mode}
              dataSource={muse.dataSource}
            />
            <ProofPanel />
            <NeuroDashboard
              fftBands={fftBands}
              eegData={muse.eegData}
              streaming={muse.connectionState.streaming}
              museConnected={muse.connectionState.connected}
              sessions={sessions}
              eegDebug={muse.eegDebug}
              displayData={displayData}
              displayConfig={displayConfig}
              onUpdateDisplayConfig={updateDisplayConfig}
              calibrationStatus={calibration.status}
              calibrationProgress={calibration.progress}
              calibrationBaseline={calibration.baseline}
              calibratedBands={calibration.calibratedBands}
              onStartCalibration={calibration.startCalibration}
              onResetCalibration={calibration.resetCalibration}
              signalQuality={signalQuality}
              recentArtifacts={artifacts.recentArtifacts}
              artifactRate={artifacts.artifactRate}
              scoreConfidence={scoreConfidence}
              simpleSessionPhase={simpleSession.phase}
              simpleSessionMode={simpleSession.mode}
              simpleSessionElapsed={simpleSession.elapsed}
              simpleSessionRemaining={simpleSession.remaining}
              simpleSessionSummary={simpleSession.summary}
              onStartSimpleSession={simpleSession.startSession}
              onEndSimpleSession={simpleSession.endSession}
              onResetSimpleSession={simpleSession.resetSession}
              nfSessionActive={nfSession.phase !== "idle"}
              onSelectProtocol={(protocol, durationMin) => {
                nfSession.selectProtocol(protocol, durationMin);
                nfSession.startCalibration();
              }}
              onSnapshotComplete={async (snapshot: BrainSnapshot) => {
                try {
                  const path = await saveSnapshot(snapshot);
                  insertEvents(snapshotToEvents(snapshot, path)).catch(() => {});
                  toast({ type: "success", title: "Snapshot Saved" });
                } catch (e) {
                  toast({ type: "error", title: "Snapshot Save Failed", description: String(e) });
                }
              }}
              replayMode={replay.mode}
              replayFiles={replay.replayFiles}
              replaySpeed={replay.speed}
              replayActiveFilename={replay.activeFilename}
              replayActiveDemoMode={replay.activeDemoMode}
              replayRecordingStartedAt={replay.recordingStartedAt}
              onStartRecording={replay.startRecording}
              onStopRecording={replay.stopRecording}
              onStartReplay={replay.startReplay}
              onStopReplay={replay.stopReplay}
              onStartDemo={replay.startDemo}
              onStopDemo={replay.stopDemo}
              onSetReplaySpeed={replay.setSpeed}
              onRefreshReplayFiles={replay.loadReplays}
              electrodeState={electrode.state}
              electrodeDerivedMode={electrode.derivedMode}
              electrodeOutputAllowed={electrode.outputAllowed}
            />
          </motion.div>
        );

      case "fitness":
        return (
          <motion.div key="fitness" {...pageTransition} className="space-y-7">
            <BlePanel
              title="Whoop 4.0"
              devices={whoop.devices}
              connectionState={whoop.connectionState}
              scanning={whoop.scanning}
              connecting={whoop.connecting}
              error={whoop.error}
              onScan={whoop.scan}
              onConnect={whoop.connect}
              onDisconnect={whoop.disconnect}
              onStartStream={whoop.startStream}
              onStopStream={whoop.stopStream}
              streaming={whoop.connectionState.streaming}
              recording={whoopRecording}
              onToggleRecord={toggleWhoopRecording}
            />
            <ProofPanel />
            <div className="grid grid-cols-3 gap-5">
              <RecoveryScore
                metrics={whoopAnalytics.recovery}
                streaming={whoop.connectionState.streaming}
              />
              <SleepChart
                metrics={whoopAnalytics.sleep}
                streaming={whoop.connectionState.streaming}
              />
              <StrainChart
                metrics={whoopAnalytics.strain}
                streaming={whoop.connectionState.streaming}
                history={[]}
              />
            </div>
            <HrvTrend
              hrData={whoop.hrData}
              rrIntervals={whoop.rrIntervals}
              currentHR={whoop.currentHR}
              stress={whoopAnalytics.stress}
            />
          </motion.div>
        );

      case "trajectory":
        return (
          <motion.div key="trajectory" {...pageTransition} className="space-y-7">
            <div className="grid grid-cols-2 gap-6">
              <GoalTracker />
              <WellnessScore
                score={{ overall: 68, recovery: 72, focus: fftBands.focusScore || 62, productivity: 71 }}
              />
            </div>
          </motion.div>
        );

      case "revenue":
        return (
          <motion.div key="revenue" {...pageTransition} className="space-y-7">
            <ArmDashboard
              strategies={arm.strategies}
              revenueSummary={arm.revenueSummary}
              riskStatus={arm.riskStatus}
              decisions={arm.decisions}
              config={arm.config}
              loading={arm.loading}
              error={arm.error}
              polling={arm.polling}
              onRefresh={arm.refresh}
              onPollWorkers={arm.pollWorkers}
              onCreateStrategy={arm.createStrategy}
              onUpdateStrategy={arm.updateStrategy}
              onDeleteStrategy={arm.deleteStrategy}
              onActivate={arm.activate}
              onPause={arm.pause}
              onKill={arm.kill}
              onKillSwitch={arm.toggleKillSwitch}
              onLogDecision={arm.logDecision}
              onUpdateOutcome={arm.updateOutcome}
            />
          </motion.div>
        );

      case "settings":
        return (
          <motion.div key="settings" {...pageTransition}>
            <SettingsPage />
          </motion.div>
        );
    }
  };

  return (
    <>
      <Shell
        terranceOnline={openclaw.status?.online}
        onRefresh={openclaw.refresh}
      >
        {(page) => (
          <AnimatePresence mode="wait">{renderPage(page)}</AnimatePresence>
        )}
      </Shell>
      <ToastContainer />
      <CommandPalette />
      <OnboardingModal />
    </>
  );
}
