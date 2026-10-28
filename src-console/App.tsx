import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { SYSTEMS } from "./app/routes";
import { useConversation } from "./app/useConversation";
import { useSnapshot } from "./app/useSnapshot";
import { useMuseSession } from "./app/useMuseSession";
import { runApproval } from "./lib/approval";
import type { TerrenceAction } from "./types";
import { NEXUS_NOTICE_EVENT } from "@/terrance/system/selfDiagnosis";
import { ActivityField, BusinessField, DevicesField, HealthField, NeuroField, RevenueField, SessionsField, SettingsField, TrajectoryField } from "./app/lazyScreens";
import { approveTerrenceAction, cancelTerrenceAction, retryTerrenceAction } from "./bridge";
import { CommandSurface } from "./design/SystemPrimitives";
import { useAppearance } from "./design/useAppearance";
import { useIdleAutoSave } from "./hooks/useIdleAutoSave";
import { useSchedulerLifecycle } from "./hooks/useSchedulerLifecycle";
import { number } from "./lib/format";
import { ChatField } from "./screens/ChatScreen";
import { TerranceField } from "./screens/TerranceScreen";
import { ContextInspector } from "./shell/ContextInspector";
import { Footer } from "./shell/Footer";
import { Sidebar } from "./shell/Sidebar";
import { ScreenSkeleton } from "./shell/ScreenSkeleton";
import { Toolbar } from "./shell/Toolbar";
import type { ModuleId } from "./types";

export default function App() {
  const [active, setActive] = useState<ModuleId>("terrance");
  const { data, busy, setBusy, refresh } = useSnapshot();
  const [notice, setNotice] = useState<string | null>(null);
  const { museDevices, museOperation, museError, sessionStartedAt, sessionEndedAt, telemetry, training, scanForMuse, connectToMuse, disconnectFromMuse, startMuseSession, stopMuseSession } = useMuseSession(data, refresh, setNotice);
  const [contextInspectorOpen, setContextInspectorOpen] = useState(false);
  const { theme, setTheme, reduceMotion, setReduceMotion } = useAppearance();
  // Terrance background scheduler: started once for the shell, stopped on unmount.
  useSchedulerLifecycle();

  const { conversationId, messages, sending, sendToTerrence, startNewConversation } = useConversation(refresh);
  // Session continuity: summarize the Terrance chat after idle / on new chat / on close (bug 6).
  const autoSave = useIdleAutoSave(messages, conversationId);
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

  const act = useCallback(async (name: string, action: () => Promise<unknown>) => { setBusy(true); setNotice(`${name} in progress`); try { await action(); setNotice(`${name} complete`); await refresh(); } catch { setBusy(false); setNotice(`${name} unavailable`); } window.setTimeout(() => setNotice(null), 3500); }, [refresh]);

  const approveAction = useCallback(async (action: TerrenceAction) => {
    await runApproval(action.title, { approve: () => approveTerrenceAction(action.id), refresh, setBusy, setNotice });
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
    : active === "chat" ? <ChatField data={data} messages={messages} sending={sending} lastSavedAt={autoSave.lastSavedAt} onSend={sendToTerrence} onNewConversation={startNewConversation} onOpenContext={() => setContextInspectorOpen(true)} busy={busy} onRetrySources={() => { void refresh(); }} onApproveAction={(action) => { void approveAction(action); }} onRetryAction={(action) => { void act(`Preparing retry for ${action.title}`, () => retryTerrenceAction(action.id)); }} onCancelAction={(action) => { void act(`Cancelling ${action.title}`, () => cancelTerrenceAction(action.id)); }} />
    : <SettingsField data={data} theme={theme} setTheme={setTheme} reduceMotion={reduceMotion} setReduceMotion={setReduceMotion} />;

  return (
    <CommandSurface mode={active}>
      <Toolbar label={selected.label} busy={busy} onRefresh={refresh} />
      <Sidebar active={active} onSelect={setActive} data={data} />
      <section className="active-field" aria-label={`${selected.label} system`}><Suspense fallback={<ScreenSkeleton />}>{view}</Suspense></section>
      <Footer data={data} busy={busy} />
      {notice && <div className="field-notice"><i />{notice}</div>}
      <ContextInspector visible={contextInspectorOpen} onClose={() => setContextInspectorOpen(false)} onChanged={refresh} />
    </CommandSurface>
  );
}
