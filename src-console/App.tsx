import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { HOST_VIEWS, SYSTEMS, resolveRoute, screenFor, type Route } from "./app/routes";
import { SegmentedControl } from "./ui";
import { useConversation } from "./app/useConversation";
import { useSnapshot } from "./app/useSnapshot";
import { useMuseSession } from "./app/useMuseSession";
import { runApproval } from "./lib/approval";
import type { TerrenceAction } from "./types";
import { NEXUS_NOTICE_EVENT } from "@/terrance/system/selfDiagnosis";
import { ActivityField, BusinessField, HealthField, NeuroField, RevenueField, SessionsField, SettingsField, retryLazyScreens } from "./app/lazyScreens";
import { ScreenErrorBoundary } from "./shell/ScreenErrorBoundary";
import { approveTerrenceAction, cancelTerrenceAction, retryTerrenceAction } from "./bridge";
import { CommandSurface } from "./design/SystemPrimitives";
import { useAppearance } from "./design/useAppearance";
import { useIdleAutoSave } from "./hooks/useIdleAutoSave";
import { useSchedulerLifecycle } from "./hooks/useSchedulerLifecycle";
import { ChatField } from "./screens/ChatScreen";
import { TerranceField } from "./screens/TerranceScreen";
import { ContextInspector } from "./shell/ContextInspector";
import { Footer } from "./shell/Footer";
import { Sidebar } from "./shell/Sidebar";
import { ScreenSkeleton } from "./shell/ScreenSkeleton";
import { Toolbar } from "./shell/Toolbar";
import type { ModuleId } from "./types";

const BUSINESS_VIEW_KEY = "amara.business.view";
function readBusinessView(): string | null {
  try { return sessionStorage.getItem(BUSINESS_VIEW_KEY); } catch { return null; }
}
function writeBusinessView(view: string) {
  try { sessionStorage.setItem(BUSINESS_VIEW_KEY, view); } catch { /* storage unavailable: choice just isn't remembered */ }
}

export default function App() {
  const [route, setRoute] = useState<Route>({ id: "terrance", view: "main" });
  const active = route.id;
  const screenId = screenFor(route);
  // §6: Trajectory folded into Home as the Goals card; the alias lands on Home and scrolls to it.
  // §9/§11: Devices and About live inside Settings; their aliases scroll to the anchor.
  useEffect(() => {
    const anchor = route.id === "terrance" && route.view === "goals" ? "goals"
      : route.id === "settings" && (route.view === "devices" || route.view === "about") ? route.view : null;
    if (!anchor) return;
    const frame = requestAnimationFrame(() => document.getElementById(anchor)?.scrollIntoView?.({ block: "start", behavior: "smooth" }));
    return () => cancelAnimationFrame(frame);
  }, [route]);
  // Old ids (sessions, trajectory, revenue, assistant, devices) resolve to their host + sub-view.
  // §7+8: the Business Work | Revenue choice persists for the session.
  const setActive = useCallback((id: ModuleId) => setRoute(() => {
    const next = resolveRoute(id);
    if (id === "business" && next.view === "main" && readBusinessView() === "revenue") return { ...next, view: "revenue" };
    return next;
  }), []);
  const { data, busy, setBusy, refresh } = useSnapshot();
  const [notice, setNotice] = useState<string | null>(null);
  const { museDevices, museOperation, museError, sessionStartedAt, sessionEndedAt, training, scanForMuse, connectToMuse, disconnectFromMuse, startMuseSession, stopMuseSession } = useMuseSession(data, refresh, setNotice);
  const [contextInspectorOpen, setContextInspectorOpen] = useState(false);
  const { theme, setTheme, reduceMotion, setReduceMotion } = useAppearance();
  // Amara background scheduler: started once for the shell, stopped on unmount.
  useSchedulerLifecycle();

  const { conversationId, messages, sending, sendToTerrence, startNewConversation } = useConversation(refresh);
  // Session continuity: summarize the Amara chat after idle / on new chat / on close (bug 6).
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
  }, [setActive]);

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
  const subViews = HOST_VIEWS[active];
  const retrySnapshot = () => { void refresh(); };
  const view = screenId === "terrance" ? <TerranceField data={data} onNavigate={setActive} onRetry={retrySnapshot} />
    : screenId === "health" ? <HealthField data={data} onRetry={retrySnapshot} />
    : screenId === "neuro" ? <NeuroField training={training} data={data} onRetry={retrySnapshot} devices={museDevices} operation={museOperation} error={museError} sessionStartedAt={sessionStartedAt} sessionEndedAt={sessionEndedAt} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onStart={() => { void startMuseSession(); }} onStop={() => { void stopMuseSession(); }} />
    : screenId === "sessions" ? <SessionsField data={data} onRetry={retrySnapshot} onStartSession={() => setRoute({ id: "neuro", view: "main" })} />
    : screenId === "business" ? <BusinessField data={data} onRetry={retrySnapshot} />
    : screenId === "revenue" ? <RevenueField data={data} onRetry={retrySnapshot} />
    : screenId === "assistant" ? <ActivityField data={data} />
    : screenId === "chat" ? <ChatField data={data} messages={messages} sending={sending} lastSavedAt={autoSave.lastSavedAt} onSend={sendToTerrence} onNewConversation={startNewConversation} onOpenContext={() => setContextInspectorOpen(true)} busy={busy} onRetrySources={retrySnapshot} onApproveAction={(action) => { void approveAction(action); }} onRetryAction={(action) => { void act(`Preparing retry for ${action.title}`, () => retryTerrenceAction(action.id)); }} onCancelAction={(action) => { void act(`Cancelling ${action.title}`, () => cancelTerrenceAction(action.id)); }} />
    : <SettingsField data={data} theme={theme} setTheme={setTheme} reduceMotion={reduceMotion} setReduceMotion={setReduceMotion} devices={museDevices} operation={museOperation} error={museError} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onRetry={retrySnapshot} />;

  return (
    <CommandSurface mode={screenId}>
      <Toolbar label={selected.label} busy={busy} onRefresh={refresh} />
      <Sidebar active={active} onSelect={setActive} data={data} />
      <section className="active-field" aria-label={`${selected.label} system`}>{subViews && <div className="subview-bar"><SegmentedControl label={`${selected.label} view`} size="small" value={route.view} options={subViews.map((entry) => ({ value: entry.view, label: entry.label }))} onChange={(view) => { if (active === "business") writeBusinessView(view); setRoute({ id: active, view }); }} /></div>}<ScreenErrorBoundary resetKey={screenId} onRetry={retryLazyScreens}><Suspense fallback={<ScreenSkeleton />}>{view}</Suspense></ScreenErrorBoundary></section>
      <Footer data={data} busy={busy} />
      {notice && <div className="field-notice"><i />{notice}</div>}
      <ContextInspector visible={contextInspectorOpen} onClose={() => setContextInspectorOpen(false)} onChanged={refresh} />
    </CommandSurface>
  );
}
