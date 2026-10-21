import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type ConversationMessage, CONVERSATION_KEY, HERMES_SESSION_KEY, HERMES_SESSION_PROFILE_KEY, NEXUS_RUNTIME_PROFILE, createConversationId, messagesFromInteractions } from "./app/conversation";
import { SYSTEMS } from "./app/routes";
import { approvalNotice } from "./lib/approval";
import type { TerrenceAction } from "./types";
import { NEXUS_NOTICE_EVENT } from "@/terrance/system/selfDiagnosis";
import { ActivityField, BusinessField, DevicesField, HealthField, NeuroField, RevenueField, SessionsField, SettingsField, TrajectoryField } from "./app/lazyScreens";
import { INITIAL_SNAPSHOT, approveTerrenceAction, cancelTerrenceAction, connectMuse, disconnectMuse, interactWithTerrence, listTerrenceInteractions, loadSystemSnapshot, migrateLegacyTerrenceContext, retryTerrenceAction, saveMuseSession, scanMuse, startEegRecording, startEegStream, stopEegRecording, stopEegStream } from "./bridge";
import { StatusIndicator } from "./design/ProductUI";
import { CommandSurface, IntelligenceOverlay } from "./design/SystemPrimitives";
import { useAppearance } from "./design/useAppearance";
import { useIdleAutoSave } from "./hooks/useIdleAutoSave";
import { useSchedulerLifecycle } from "./hooks/useSchedulerLifecycle";
import { tone, number, time, readableError } from "./lib/format";
import { SessionFinalizer } from "./neuro/sessionFinalizer";
import { useMuseTelemetry } from "./neuro/useMuseTelemetry";
import { useTraining } from "./neuro/useTraining";
import { TerranceField } from "./screens/TerranceScreen";
import type { MuseOperation } from "./screens/muse";
import { ContextInspector } from "./shell/ContextInspector";
import { Footer } from "./shell/Footer";
import { Sidebar } from "./shell/Sidebar";
import { Toolbar } from "./shell/Toolbar";
import type { ModuleId, MuseDevice, RecordedSession, SystemSnapshot } from "./types";
import { RefreshCw, Zap } from "lucide-react";

export default function App() {
  const [active, setActive] = useState<ModuleId>("terrance");
  const [data, setData] = useState<SystemSnapshot>(INITIAL_SNAPSHOT);
  const [busy, setBusy] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [contextInspectorOpen, setContextInspectorOpen] = useState(false);
  const [museDevices, setMuseDevices] = useState<MuseDevice[]>([]);
  const [museOperation, setMuseOperation] = useState<MuseOperation>(null);
  const [museError, setMuseError] = useState<string | null>(null);
  const [sessionStartedAt, setSessionStartedAt] = useState<number | null>(null);
  const [recordingFile, setRecordingFile] = useState<string | null>(null);
  const [sessionEndedAt, setSessionEndedAt] = useState<number | null>(null);
  const sessionFinalizer = useRef<SessionFinalizer<{ session: RecordedSession & { file_path: string }; data: Record<string, unknown> }> | null>(null);
  const sessionOperation = useRef(false);
  const sessionStopTime = useRef<number | null>(null);
  const { telemetry, reset: resetTelemetry, readCounters } = useMuseTelemetry();
  const training = useTraining(telemetry, sessionStartedAt !== null && sessionEndedAt === null && data.muse.streaming);
  const { theme, setTheme, reduceMotion, setReduceMotion } = useAppearance();
  const [conversationId, setConversationId] = useState(() => {
    const existing = window.localStorage.getItem(CONVERSATION_KEY);
    if (existing) return existing;
    const created = createConversationId();
    window.localStorage.setItem(CONVERSATION_KEY, created);
    return created;
  });
  const [hermesSessionId, setHermesSessionId] = useState<string | null>(() => (
    window.localStorage.getItem(HERMES_SESSION_PROFILE_KEY) === NEXUS_RUNTIME_PROFILE
      ? window.localStorage.getItem(HERMES_SESSION_KEY)
      : null
  ));
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [sending, setSending] = useState(false);
  // Session continuity: summarize the Terrance chat after idle / on new chat / on close (bug 6).
  // Move with the chat state when the shell is split.
  useIdleAutoSave(messages, conversationId);
  // Terrance background scheduler: started once for the shell, stopped on unmount.
  useSchedulerLifecycle();

  const refresh = useCallback(async () => { setBusy(true); setData(await loadSystemSnapshot()); setBusy(false); }, []);
  useEffect(() => { refresh(); const refreshTimer = window.setInterval(refresh, 15_000); return () => { window.clearInterval(refreshTimer); }; }, [refresh]);
  useEffect(() => {
    let activeRequest = true;
    migrateLegacyTerrenceContext()
      .then(() => {
        if (activeRequest) return refresh();
      })
      .catch((error) => console.warn("[console] legacy context migration unavailable", error));
    return () => { activeRequest = false; };
  }, [refresh]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!event.metaKey || event.ctrlKey || event.altKey || !/^[1-9]$/.test(event.key)) return;
      const index = Number(event.key) - 1;
      if (index < 0 || index >= Math.min(9, SYSTEMS.length)) return;
      event.preventDefault();
      setActive(SYSTEMS[index].id);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  useEffect(() => {
    let activeRequest = true;
    listTerrenceInteractions(conversationId).then((interactions) => {
      if (!activeRequest) return;
      setMessages(messagesFromInteractions(interactions));
      const latestSession = [...interactions]
        .reverse()
        .find((interaction) => interaction.runtimeProfile === NEXUS_RUNTIME_PROFILE && interaction.hermesSessionId)
        ?.hermesSessionId;
      if (latestSession) {
        setHermesSessionId(latestSession);
        window.localStorage.setItem(HERMES_SESSION_KEY, latestSession);
        window.localStorage.setItem(HERMES_SESSION_PROFILE_KEY, NEXUS_RUNTIME_PROFILE);
      }
    }).catch((error) => console.warn("[console] interaction history unavailable", error));
    return () => { activeRequest = false; };
  }, [conversationId]);

  const act = useCallback(async (name: string, action: () => Promise<unknown>) => { setBusy(true); setNotice(`${name} in progress`); try { await action(); setNotice(`${name} complete`); await refresh(); } catch { setBusy(false); setNotice(`${name} unavailable`); } window.setTimeout(() => setNotice(null), 3500); }, [refresh]);

  const approveAction = useCallback(async (action: TerrenceAction) => {
    setBusy(true);
    setNotice(`Approving ${action.title} in progress`);
    try {
      const result = await approveTerrenceAction(action.id);
      setNotice(approvalNotice(action.title, { result }));
      await refresh();
    } catch (error) {
      setBusy(false);
      setNotice(approvalNotice(action.title, { error }));
    }
    window.setTimeout(() => setNotice(null), 3500);
  }, [refresh]);

  // Quiet in-app notices from background work (e.g. auto-heal "Restart declined").
  useEffect(() => {
    let timer: number | undefined;
    const onNotice = (event: Event) => {
      const message = (event as CustomEvent<{ message?: string }>).detail?.message;
      if (!message) return;
      setNotice(message);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setNotice(null), 3500);
    };
    window.addEventListener(NEXUS_NOTICE_EVENT, onNotice);
    return () => { window.removeEventListener(NEXUS_NOTICE_EVENT, onNotice); window.clearTimeout(timer); };
  }, []);

  const scanForMuse = useCallback(async () => {
    setMuseOperation("scanning");
    setMuseError(null);
    try {
      const found = await scanMuse();
      setMuseDevices(found);
      setNotice(found.length ? `${found.length} Muse headset${found.length === 1 ? "" : "s"} found` : "No Muse headsets found");
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
      await refresh();
    }
  }, [refresh]);

  const connectToMuse = useCallback(async (device: MuseDevice) => {
    if (sessionStartedAt !== null || sessionOperation.current) {
      setMuseError("Stop and save the current session in Live before connecting another headset.");
      return;
    }
    setMuseOperation("connecting");
    setMuseError(null);
    try {
      await connectMuse(device.address);
      setMuseDevices([]);
      setNotice(`${device.name} connected`);
      await refresh();
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [refresh, sessionStartedAt]);

  const persistMuseSession = useCallback(async () => {
    if (sessionStartedAt === null || !recordingFile) {
      await stopEegStream();
      await stopEegRecording();
      return false;
    }
    const finalizer = sessionFinalizer.current ??= new SessionFinalizer();
    await finalizer.run({
      stopStream: async () => {
        await stopEegStream();
        sessionStopTime.current = Date.now();
        setSessionEndedAt(sessionStopTime.current);
      },
      stopRecording: stopEegRecording,
      snapshot: () => {
        const endedAt = sessionStopTime.current ?? Date.now();
        const counters = readCounters();
        const trainingSummary = training.summary();
        const session: RecordedSession & { file_path: string } = {
          id: typeof crypto?.randomUUID === "function" ? crypto.randomUUID() : `muse-${sessionStartedAt}`,
          device_type: "muse",
          start: new Date(sessionStartedAt).toISOString(),
          end: new Date(endedAt).toISOString(),
          duration_secs: Math.max(1, Math.round((endedAt - sessionStartedAt) / 1000)),
          data_points: counters.sampleCount,
          file_path: recordingFile,
          summary: { avg_focus: trainingSummary?.avg_focus ?? null, avg_relaxation: trainingSummary?.avg_relaxation ?? null, avg_hr: null, max_hr: null, avg_hrv: null },
        };
        training.finish();
        return { session, data: {
          source: "live_muse",
          replay_file: recordingFile,
          packets: counters.packetCount,
          channel_samples: counters.channelSamples,
          neurofeedback: trainingSummary,
        } };
      },
      save: (payload) => saveMuseSession(payload.session, payload.data),
    });
    return true;
  }, [recordingFile, sessionStartedAt, readCounters, training.summary, training.finish]);

  const startMuseSession = useCallback(async () => {
    if (sessionOperation.current || sessionStartedAt !== null || sessionFinalizer.current) return;
    sessionOperation.current = true;
    setMuseOperation("starting");
    setMuseError(null);
    resetTelemetry();
    let file: string | null = null;
    try {
      file = await startEegRecording();
      await startEegStream();
      const startedAt = Date.now();
      setRecordingFile(file);
      sessionStopTime.current = null;
      setSessionStartedAt(startedAt);
      training.begin();
      setNotice("Muse session started");
      await refresh();
    } catch (error) {
      if (file) await stopEegRecording().catch(() => undefined);
      setMuseError(readableError(error));
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [refresh, resetTelemetry, training.begin, sessionStartedAt]);

  const stopMuseSession = useCallback(async () => {
    if (sessionOperation.current) return;
    sessionOperation.current = true;
    setMuseOperation("stopping");
    setMuseError(null);
    try {
      const saved = await persistMuseSession();
      training.finish();
      setSessionStartedAt(null);
      setRecordingFile(null);
      setSessionEndedAt(null);
      sessionFinalizer.current = null;
      setNotice(saved ? "Muse session saved" : "Muse stream stopped");
    } catch (error) {
      setMuseError(`Session could not be finalized. Keep Nexus open and retry. ${readableError(error)}`);
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      await refresh();
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [persistMuseSession, refresh, training.finish]);

  const disconnectFromMuse = useCallback(async () => {
    if (sessionOperation.current) return;
    sessionOperation.current = true;
    setMuseOperation("disconnecting");
    setMuseError(null);
    try {
      if (data.muse.streaming || sessionStartedAt !== null) {
        await persistMuseSession();
      }
      await disconnectMuse();
      training.finish();
      setSessionStartedAt(null);
      setRecordingFile(null);
      setSessionEndedAt(null);
      sessionFinalizer.current = null;
      resetTelemetry();
      setNotice("Muse disconnected");
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      await refresh();
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [data.muse.streaming, sessionStartedAt, persistMuseSession, refresh, resetTelemetry, training.finish]);
  const sendToTerrence = useCallback(async (message: string) => {
    const requestId = `pending-${Date.now()}`;
    setMessages((current) => [...current, { id: `${requestId}-user`, role: "user", content: message, timestamp: new Date().toISOString() }]);
    setSending(true);
    try {
      const interaction = await interactWithTerrence({ message, conversationId, hermesSessionId });
      if (interaction.hermesSessionId) {
        setHermesSessionId(interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_KEY, interaction.hermesSessionId);
        window.localStorage.setItem(HERMES_SESSION_PROFILE_KEY, interaction.runtimeProfile);
      }
      setMessages((current) => [...current, ...messagesFromInteractions([interaction]).filter((entry) => entry.role === "assistant")]);
      await refresh();
    } catch (error) {
      setMessages((current) => [...current, {
        id: `${requestId}-failed`,
        role: "assistant",
        content: error instanceof Error ? error.message : String(error),
        timestamp: new Date().toISOString(),
        status: "failed",
      }]);
    } finally {
      setSending(false);
    }
  }, [conversationId, hermesSessionId, refresh]);
  const startNewConversation = useCallback(() => {
    const created = createConversationId();
    window.localStorage.setItem(CONVERSATION_KEY, created);
    window.localStorage.removeItem(HERMES_SESSION_KEY);
    window.localStorage.removeItem(HERMES_SESSION_PROFILE_KEY);
    setHermesSessionId(null);
    setMessages([]);
    setConversationId(created);
  }, []);
  const selected = useMemo(() => SYSTEMS.find((system) => system.id === active)!, [active]);
  const view = active === "terrance" ? <TerranceField data={data} messages={messages} sending={sending} onSend={sendToTerrence} onNewConversation={startNewConversation} onOpenContext={() => setContextInspectorOpen(true)} busy={busy} onApproveAction={(action) => { void approveAction(action); }} onRetryAction={(action) => { void act(`Preparing retry for ${action.title}`, () => retryTerrenceAction(action.id)); }} onCancelAction={(action) => { void act(`Cancelling ${action.title}`, () => cancelTerrenceAction(action.id)); }} />
    : active === "health" ? <HealthField data={data} />
    : active === "neuro" ? <NeuroField training={training} data={data} devices={museDevices} telemetry={telemetry} operation={museOperation} error={museError} sessionStartedAt={sessionStartedAt} sessionEndedAt={sessionEndedAt} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onStart={() => { void startMuseSession(); }} onStop={() => { void stopMuseSession(); }} />
    : active === "sessions" ? <SessionsField data={data} />
    : active === "devices" ? <DevicesField data={data} devices={museDevices} operation={museOperation} error={museError} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} />
    : active === "business" ? <BusinessField data={data} />
    : active === "revenue" ? <RevenueField data={data} />
    : active === "assistant" ? <ActivityField data={data} />
    : active === "trajectory" ? <TrajectoryField data={data} />
    : <SettingsField data={data} theme={theme} setTheme={setTheme} reduceMotion={reduceMotion} setReduceMotion={setReduceMotion} />;

  return (
    <CommandSurface mode={active}>
      <Toolbar label={selected.label} busy={busy} onRefresh={refresh} />
      <Sidebar active={active} onSelect={setActive} data={data} />
      <section className="active-field" aria-label={`${selected.label} system`}><Suspense fallback={null}>{view}</Suspense></section>
      <Footer data={data} busy={busy} />
      {notice && <div className="field-notice"><i />{notice}</div>}
      <ContextInspector visible={contextInspectorOpen} onClose={() => setContextInspectorOpen(false)} onChanged={refresh} />
    </CommandSurface>
  );
}
