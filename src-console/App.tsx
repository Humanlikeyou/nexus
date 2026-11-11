import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { HOST_VIEWS, SECTION_ANCHORS, SYSTEMS, TODAY_LABEL, TODAY_ROUTE, resolveRoute, screenFor } from "./app/routes";
import { PageSection } from "./ui";
import { AgentsField } from "./screens/AgentsScreen";
import { useVoiceTurns } from "./hooks/useVoiceTurns";
import { useRouteNavigation } from "./app/useRouteNavigation";
import { SegmentedControl } from "./ui";
import { useConversation } from "./app/useConversation";
import { useSnapshot } from "./app/useSnapshot";
import { useMuseSession } from "./app/useMuseSession";
import { runApproval } from "./lib/approval";
import type { TerrenceAction } from "./types";
import { NEXUS_NOTICE_EVENT } from "@/terrance/system/selfDiagnosis";
import { ActivityField, BusinessField, CrewsField, ListsField, HealthField, MindBodyField, NeuroField, SmartHomeField, RevenueField, SessionsField, SettingsField, VoiceField, retryLazyScreens } from "./app/lazyScreens";
import { ScreenErrorBoundary } from "./shell/ScreenErrorBoundary";
import { INITIAL_SNAPSHOT, approveTerrenceAction, cancelTerrenceAction, retryTerrenceAction } from "./bridge";
import { CommandSurface } from "./design/SystemPrimitives";
import { useAppearance } from "./design/useAppearance";
import { useWindowActive } from "./shell/useWindowActive";
import { useIdleAutoSave } from "./hooks/useIdleAutoSave";
import { useSchedulerLifecycle } from "./hooks/useSchedulerLifecycle";
import { ChatField } from "./screens/ChatScreen";
import { AmaraVoiceAnnouncer } from "./components/AmaraLens/AmaraLens";
import { LandingField } from "./screens/LandingScreen";
import { TerranceField } from "./screens/TerranceScreen";
import { ContextInspector } from "./shell/ContextInspector";
import { Footer } from "./shell/Footer";
import { Sidebar } from "./shell/Sidebar";
import { ScreenSkeleton } from "./shell/ScreenSkeleton";
import { Toolbar } from "./shell/Toolbar";
import { CommandPalette } from "./shell/CommandPalette";
import { Splash, reducedMotion } from "./shell/Splash";
import type { ModuleId } from "./types";
import { personalAdapter } from "./personal/adapters";
import { PatternsView } from "./personal/PageViews";
import { ASK_MORE_EVENT, type AskMoreDetail } from "./amaraTake/takeView";
import { standaloneAdapter } from "./standalone/adapters";
import { FirstRun } from "./standalone/FirstRun";
import { settingsTab } from "./app/settingsTabs";

const BUSINESS_VIEW_KEY = "amara.business.view";
function readBusinessView(): string | null {
  try { return sessionStorage.getItem(BUSINESS_VIEW_KEY); } catch { return null; }
}
function writeBusinessView(view: string) {
  try { sessionStorage.setItem(BUSINESS_VIEW_KEY, view); } catch { /* storage unavailable: choice just isn't remembered */ }
}

const INITIAL_ROUTE = { id: "terrance", view: "main" } as const;

export default function App() {
  const { route, navigate } = useRouteNavigation(INITIAL_ROUTE);
  const active = route.id;
  const screenId = screenFor(route);
  // §6: Trajectory folded into Home as the Goals card; the alias lands on Home and scrolls to it.
  // §9/§11: Devices and About live inside Settings; their aliases scroll to the anchor.
  useEffect(() => {
    // design-1110b B: Lists (Today), Activity (Agents) and Voice (Settings) are sections too.
    const anchor = SECTION_ANCHORS[route.id]?.includes(route.view) ? route.view : null;
    if (!anchor) return;
    // The host screen is lazy: retry for ~1s of frames until the anchor has mounted.
    let frame = 0, tries = 0;
    const scroll = () => {
      const el = document.getElementById(anchor);
      const fold = el?.closest("details");
      if (fold) fold.open = true; // an anchor can live behind a page's disclosure
      if (el) el.scrollIntoView?.({ block: "start", behavior: reducedMotion() ? "auto" : "smooth" });
      else if (tries++ < 60) frame = requestAnimationFrame(scroll);
    };
    frame = requestAnimationFrame(scroll);
    return () => cancelAnimationFrame(frame);
  }, [route]);
  // Old ids (health, neuro, sessions, trajectory, revenue, assistant, devices) resolve to their host + sub-view.
  // §7+8: the Business Work | Revenue choice persists for the session.
  const setActive = useCallback((id: ModuleId) => {
    const next = resolveRoute(id);
    navigate(id === "business" && next.view === "main" && readBusinessView() === "revenue" ? { ...next, view: "revenue" } : next);
  }, [navigate]);
  const { data, busy, setBusy, refresh } = useSnapshot();
  const [notice, setNotice] = useState<string | null>(null);
  const { museDevices, museOperation, museError, sessionStartedAt, sessionEndedAt, training, scanForMuse, connectToMuse, disconnectFromMuse, startMuseSession, stopMuseSession } = useMuseSession(data, refresh, setNotice);
  const [contextInspectorOpen, setContextInspectorOpen] = useState(false);
  const { theme, setTheme, reduceMotion, setReduceMotion } = useAppearance();
  // Key-window state → <html data-window-active> (Decision 21).
  useWindowActive();
  // Amara background scheduler: started once for the shell, stopped on unmount.
  useSchedulerLifecycle();

  const { conversationId, messages, sending, loadingModel, sendToTerrence, startNewConversation, cancelPending } = useConversation(refresh);
  // Session continuity: summarize the Amara chat after idle / on new chat / on close (bug 6).
  const autoSave = useIdleAutoSave(messages, conversationId);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [askDraft, setAskDraft] = useState<{ text: string; n: number }>({ text: "", n: 0 });
  // design-1110f "Ask more ›": Home chat opens with the take as context and its question pre-filled; nothing is sent.
  const [askContext, setAskContext] = useState<AskMoreDetail | null>(null);
  useEffect(() => {
    const onAsk = (event: Event) => {
      const detail = (event as CustomEvent<AskMoreDetail>).detail;
      if (!detail) return;
      setAskContext(detail);
      setAskDraft((d) => ({ text: detail.question, n: d.n + 1 }));
      navigate({ id: "terrance", view: "main" });
    };
    window.addEventListener(ASK_MORE_EVENT, onAsk);
    return () => window.removeEventListener(ASK_MORE_EVENT, onAsk);
  }, [navigate]);
  // design-1110b A: the Home chat. Expand is the largest it gets; ⌘1 focuses its composer.
  const [chatExpanded, setChatExpanded] = useState(false);
  // design-1111s first-run: opens on launch only when setup_state is live and not done; any time from Settings › General.
  const [setup, setSetup] = useState<null | "first" | "again">(() => { const d = standaloneAdapter().setupDone(); return d.state === "live" && !d.value ? "first" : null; });
  const [chatFocus, setChatFocus] = useState(0);
  const voiceTurns = useVoiceTurns(screenId === "landing");
  useEffect(() => {
    const palette = (event: KeyboardEvent) => {
      if (!event.metaKey || event.ctrlKey || event.altKey || event.key.toLowerCase() !== "k") return;
      event.preventDefault();
      setPaletteOpen((open) => !open);
    };
    window.addEventListener("keydown", palette);
    return () => window.removeEventListener("keydown", palette);
  }, []);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!event.metaKey || event.ctrlKey || event.altKey || !/^[1-9]$/.test(event.key)) return;
      const index = Number(event.key) - 1;
      if (index < 0 || index >= Math.min(9, SYSTEMS.length)) return;
      event.preventDefault();
      setActive(SYSTEMS[index].id);
      if (index === 0) setChatFocus((n) => n + 1);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [setActive]);

  const act = useCallback(async (name: string, action: () => Promise<unknown>) => { setBusy(true); setNotice(`${name} in progress`); try { await action(); setNotice(`${name} complete`); await refresh(); } catch { setBusy(false); setNotice(`${name} unavailable`); } window.setTimeout(() => setNotice(null), 3500); }, [refresh, setBusy]);

  const approveAction = useCallback(async (action: TerrenceAction) => {
    await runApproval(action.title, { approve: () => approveTerrenceAction(action.id), refresh, setBusy, setNotice });
    window.setTimeout(() => setNotice(null), 3500);
  }, [refresh, setBusy]);

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
  const onLanding = screenId === "landing";
  const fieldLabel = screenId === "terrance" ? TODAY_LABEL : selected.label;
  const retrySnapshot = () => { void refresh(); };
  const chat = <ChatField key={askDraft.n} variant="home" expanded={chatExpanded} onToggleExpanded={() => setChatExpanded((open) => !open)} voiceTurns={voiceTurns} focusSignal={chatFocus + askDraft.n}
    conversationId={conversationId} initialDraft={askDraft.text} data={data} messages={messages} sending={sending} loadingModel={loadingModel} onCancelPending={cancelPending} lastSavedAt={autoSave.lastSavedAt} onSend={sendToTerrence} onNewConversation={startNewConversation} onOpenContext={() => setContextInspectorOpen(true)} busy={busy} onRetrySources={retrySnapshot} onApproveAction={(action) => { void approveAction(action); }} onRetryAction={(action) => { void act(`Preparing retry for ${action.title}`, () => retryTerrenceAction(action.id)); }} onCancelAction={(action) => { void act(`Cancelling ${action.title}`, () => cancelTerrenceAction(action.id)); }} askContext={askContext} onClearContext={() => setAskContext(null)} />;
  const view = onLanding ? <LandingField onSelect={setActive} onToday={() => navigate(TODAY_ROUTE)} chat={chat} chatExpanded={chatExpanded} />
    : screenId === "terrance" ? <><TerranceField data={data} onNavigate={setActive} onRetry={retrySnapshot} /><PageSection id="lists"><ListsField refreshSignal={data.refreshedAt} /></PageSection></>
    : screenId === "health" ? <HealthField data={data} onRetry={retrySnapshot} />
    : screenId === "mindbody" ? (route.view === "patterns" ? <PatternsView adapter={personalAdapter()} /> : <MindBodyField sessions={data.sessions} />)
    : screenId === "smarthome" ? <SmartHomeField onOpenSettings={() => navigate({ id: "settings", view: "home-assistant" })} />
    : screenId === "neuro" ? <NeuroField training={training} data={data} onRetry={retrySnapshot} onOpenHistory={() => setActive("sessions")} devices={museDevices} operation={museOperation} error={museError} sessionStartedAt={sessionStartedAt} sessionEndedAt={sessionEndedAt} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onStart={() => { void startMuseSession(); }} onStop={() => { void stopMuseSession(); }} />
    : screenId === "sessions" ? <SessionsField data={data} onRetry={retrySnapshot} onStartSession={() => setActive("neuro")} />
    : screenId === "business" ? <BusinessField data={data} onRetry={retrySnapshot} view={route.view} onView={(view) => navigate({ id: "business", view })} />
    : screenId === "revenue" ? <RevenueField data={data} onRetry={retrySnapshot} />
    : screenId === "assistant" ? <ActivityField data={data} onRetry={retrySnapshot} />
    : screenId === "lists" ? <ListsField refreshSignal={data.refreshedAt} />
    : screenId === "crews" ? <CrewsField data={data} onRetry={retrySnapshot} />
    : screenId === "voice" ? <VoiceField />
    : screenId === "chat" ? <><AgentsField data={data} onOpenCrews={() => navigate({ id: "chat", view: "crews" })} onRetry={retrySnapshot} /><PageSection id="activity"><ActivityField data={data} onRetry={retrySnapshot} /></PageSection></>
    : <SettingsField view={route.view} onRunSetup={() => setSetup("again")} data={data} theme={theme} setTheme={setTheme} reduceMotion={reduceMotion} setReduceMotion={setReduceMotion} devices={museDevices} operation={museOperation} error={museError} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onRetry={retrySnapshot} />;

  return (
    <CommandSurface mode={screenId}>
      {!onLanding && <Toolbar label={fieldLabel} busy={busy} onRefresh={refresh} onSearch={() => setPaletteOpen(true)} />}
      <Splash loaded={data.refreshedAt !== INITIAL_SNAPSHOT.refreshedAt} />
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} onNavigate={navigate}
        onAsk={(text) => { setAskContext(null); setAskDraft((d) => ({ text, n: d.n + 1 })); navigate({ id: "terrance", view: "main" }); }} />
      {!onLanding && <Sidebar active={active} onSelect={setActive} data={data} onOpenServices={() => navigate({ id: "settings", view: "services" })} />}
      <section className="active-field" aria-label={`${fieldLabel} system`}>{subViews && <div className="subview-bar" data-host={active}><SegmentedControl label={`${selected.label} view`} size="small" value={active === "settings" ? settingsTab(route.view) : subViews.some((entry) => entry.view === route.view) ? route.view : "main"} options={subViews.map((entry) => ({ value: entry.view, label: entry.label }))} onChange={(view) => { if (active === "business") writeBusinessView(view); navigate({ id: active, view }); }} /></div>}<ScreenErrorBoundary resetKey={screenId} onRetry={retryLazyScreens}><Suspense fallback={<ScreenSkeleton />}>{view}</Suspense></ScreenErrorBoundary></section>
      {!onLanding && <Footer data={data} busy={busy} />}
      {!onLanding && <AmaraVoiceAnnouncer />}
      {notice && <div className="field-notice"><i />{notice}</div>}
      {setup && <FirstRun adapter={standaloneAdapter()} again={setup === "again"} onClose={() => setSetup(null)} />}
      <ContextInspector visible={contextInspectorOpen} onClose={() => setContextInspectorOpen(false)} onChanged={refresh} />
    </CommandSurface>
  );
}
