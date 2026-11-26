import { Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { HOST_VIEWS, SECTION_ANCHORS, goalRoute, SYSTEMS, TODAY_LABEL, TODAY_ROUTE, canonicalRoute, crewRunOf, resolveRoute, screenFor, tabOf, type Route } from "./app/routes";
import { PageSection } from "./ui";
import { AgentsField } from "./screens/AgentsScreen";
import { useChatAdds } from "./adds/useChatAdds";
import { useVoiceTurns } from "./hooks/useVoiceTurns";
import { useRouteNavigation } from "./app/useRouteNavigation";
import { SegmentedControl } from "./ui";
import { useConversation } from "./app/useConversation";
import { useScreenOpens } from "./app/useScreenOpens";
import { useSnapshot } from "./app/useSnapshot";
import { useMuseSession } from "./app/useMuseSession";
import { runApproval } from "./lib/approval";
import type { TerrenceAction } from "./types";
import { NEXUS_NOTICE_EVENT } from "./legacy-core/terrance/system/selfDiagnosis";
import { ActivityField, BusinessField, CrewsField, LifeField, ListsField, HealthField, MindBodyField, NeuroField, SmartHomeField, RevenueField, SessionsField, SettingsField, VoiceField, retryLazyScreens } from "./app/lazyScreens";
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
import { OPEN_GOAL_EVENT } from "./goals/openGoal";
import { FOCUS_ACTION_EVENT } from "./inbox/needsYou";
import { standaloneAdapter } from "./standalone/adapters";
import { FirstRun } from "./standalone/FirstRun";
import { settingsTab } from "./app/settingsTabs";
import { applyIntent, handleWindowPrompt, useVoiceWindowPrompts } from "./homeWindows/useHomeWindows";
import { homeWindowsStore, resolveVoiceTarget } from "./homeWindows";
import { createSendHome } from "./app/turnChannel";
import { enterNavy, exitNavy, handleNavyPrompt, NavyRoot, useVoiceNavyPrompts } from "./navyMode";

const BUSINESS_VIEW_KEY = "amara.business.view";
function readBusinessView(): string | null {
  try { return sessionStorage.getItem(BUSINESS_VIEW_KEY); } catch { return null; }
}
function writeBusinessView(view: string) {
  try { sessionStorage.setItem(BUSINESS_VIEW_KEY, view); } catch { /* storage unavailable: choice just isn't remembered */ }
}

/** "neuro-history/<session id>": the recording that Neuro, the journey chips or a link opened. */
const historyIdOf = (view: string): string | null => (view.startsWith("neuro-history/") ? view.slice("neuro-history/".length) || null : null);

const INITIAL_ROUTE = { id: "terrance", view: "main" } as const;

export default function App() {
  const { route, navigate } = useRouteNavigation(INITIAL_ROUTE);
  const active = route.id;
  useScreenOpens(active);
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
  // §7+8: the Enterprise Overview | Finances choice persists for the session (design-1111v: the stored
  // "revenue" view now means Finances; a stored view that moved out of Enterprise, like "personal", is ignored).
  const setActive = useCallback((id: ModuleId) => {
    const next = resolveRoute(id);
    const stored = readBusinessView();
    const restored = next.id === "enterprise" && next.view === "main" && stored ? canonicalRoute({ id: "enterprise", view: stored }) : null;
    navigate(restored && restored.id === "enterprise" && restored.view === "finances" ? restored : next);
  }, [navigate]);
  const { data, busy, setBusy, refresh } = useSnapshot();
  const [notice, setNotice] = useState<string | null>(null);
  const [noticeLink, setNoticeLink] = useState<{ label: string; route: Route } | null>(null);
  const { museDevices, museOperation, museError, scanEmpty, lastSavedId, dismissSaved, sessionId, sessionStartedAt, sessionEndedAt, training, scanForMuse, connectToMuse, disconnectFromMuse, startMuseSession, stopMuseSession } = useMuseSession(data, refresh, setNotice);
  // Neuro history › "Start session": Neuro opens with Find Muse or Start session focused.
  const [neuroFocus, setNeuroFocus] = useState(0);
  const [contextInspectorOpen, setContextInspectorOpen] = useState(false);
  const { theme, setTheme, reduceMotion, setReduceMotion } = useAppearance();
  // Key-window state → <html data-window-active> (Decision 21).
  useWindowActive();
  // Amara background scheduler: started once for the shell, stopped on unmount.
  useSchedulerLifecycle();

  const { conversationId, messages, sending, loadingModel, sendToTerrence, startNewConversation, cancelPending, postLocal } = useConversation(refresh);
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
  // "Open goal ›" from the step question in a take: Life › Goals with that goal in focus.
  useEffect(() => {
    const onGoal = (event: Event) => { const id = (event as CustomEvent<{ goalId?: string }>).detail?.goalId; if (id) navigate(goalRoute(id)); };
    window.addEventListener(OPEN_GOAL_EVENT, onGoal);
    return () => window.removeEventListener(OPEN_GOAL_EVENT, onGoal);
  }, [navigate]);
  // design-1110b A: the Home chat. Expand is the largest it gets; ⌘1 focuses its composer.
  const [chatExpanded, setChatExpanded] = useState(false);
  // Needs you › "Open on Home" for an Amara approval: expand the chat; it scrolls to the card and focuses Approve.
  const [focusAction, setFocusAction] = useState<{ id: string; n: number } | null>(null);
  useEffect(() => {
    const onFocus = (event: Event) => {
      const id = (event as CustomEvent<{ id?: string }>).detail?.id;
      if (!id) return;
      setChatExpanded(true);
      setFocusAction((f) => ({ id, n: (f?.n ?? 0) + 1 }));
      navigate({ id: "terrance", view: "main" });
    };
    window.addEventListener(FOCUS_ACTION_EVENT, onFocus);
    return () => window.removeEventListener(FOCUS_ACTION_EVENT, onFocus);
  }, [navigate]);
  // design-1111s first-run: opens on launch only when setup_state is live and not done; any time from Settings › General.
  const [setup, setSetup] = useState<null | "first" | "again">(() => { const d = standaloneAdapter().setupDone(); return d.state === "live" && !d.value ? "first" : null; });
  const [chatFocus, setChatFocus] = useState(0);
  // A question typed in the Home windows command bar: the chat is hidden behind the windows, so its reply shows in the bar.
  const [windowAsk, setWindowAsk] = useState<{ base: number } | null>(null);
  const voiceTurns = useVoiceTurns(screenId === "landing");
  useVoiceNavyPrompts(voiceTurns);
  useVoiceWindowPrompts(voiceTurns);
  const { handleTyped: handleChatAdd } = useChatAdds(postLocal, voiceTurns);
  // Flag off: the old renderer pre-route and send. Flag on (nexus.turnPipeline): Rust owns adds, navy, window open and HA.
  const sendHome = useMemo(() => createSendHome({
    handleChatAdd, handleNavyPrompt, handleWindowPrompt, enterNavy, exitNavy, sendToTerrence,
    resolveWindowIntent: (message) => resolveVoiceTarget(homeWindowsStore.get(), message),
    applyWindowIntent: applyIntent,
  }), [sendToTerrence, handleChatAdd]);
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
  // Toolbar Refresh: say how it went once the snapshot is back (G14).
  const [refreshing, setRefreshing] = useState(false);
  const manualRefresh = () => { setRefreshing(true); void refresh(); };
  useEffect(() => {
    if (!refreshing || busy) return;
    const down = data.unavailableSources.length;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- reports the finished refresh once
    setRefreshing(false);
    setNotice("Refreshed");
    setNoticeLink(down === 0 ? null : { label: "Some sources unavailable ›", route: { id: "settings", view: "about" } });
    const t = window.setTimeout(() => { setNotice(null); setNoticeLink(null); }, 6000);
    return () => window.clearTimeout(t);
  }, [refreshing, busy, data]);
  const retrySnapshot = () => { void refresh(); };
  const windowReplyText = windowAsk ? messages.slice(windowAsk.base).find((m) => m.role === "assistant")?.content ?? null : null;
  const windowReply = windowAsk ? { pending: sending && windowReplyText == null, reply: windowReplyText } : null;
  const chat = <ChatField key={askDraft.n} variant="home" expanded={chatExpanded} onToggleExpanded={() => setChatExpanded((open) => !open)} voiceTurns={voiceTurns} focusSignal={chatFocus + askDraft.n}
    conversationId={conversationId} initialDraft={askDraft.text} data={data} messages={messages} sending={sending} loadingModel={loadingModel} onCancelPending={cancelPending} lastSavedAt={autoSave.lastSavedAt} onSend={sendHome} onNewConversation={startNewConversation} onOpenContext={() => setContextInspectorOpen(true)} busy={busy} onRetrySources={retrySnapshot} onApproveAction={(action) => { void approveAction(action); }} onRetryAction={(action) => { void act(`Preparing retry for ${action.title}`, () => retryTerrenceAction(action.id)); }} onCancelAction={(action) => { void act(`Cancelling ${action.title}`, () => cancelTerrenceAction(action.id)); }} askContext={askContext} onClearContext={() => setAskContext(null)} focusAction={focusAction} />;
  const view = onLanding ? <LandingField onSelect={setActive} onToday={() => navigate(TODAY_ROUTE)} chat={chat} chatExpanded={chatExpanded} onAsk={(text) => { setWindowAsk({ base: messages.length }); void sendHome(text); }} onOpen={navigate} windowAsk={windowReply} onOpenChat={() => setWindowAsk(null)} />
    : screenId === "terrance" ? <TerranceField data={data} onNavigate={setActive} onOpen={navigate} onRetry={retrySnapshot} todos={<PageSection id="lists"><ListsField refreshSignal={data.refreshedAt} /></PageSection>} />
    : screenId === "health" ? <HealthField data={data} onRetry={retrySnapshot} onOpen={navigate} />
    : screenId === "mindbody" ? (route.view === "patterns" ? <PatternsView adapter={personalAdapter()} /> : <MindBodyField sessions={data.sessions} onOpen={navigate} />)
    : screenId === "smarthome" ? <SmartHomeField onOpenSettings={() => navigate({ id: "settings", view: "home-assistant" })} />
    : screenId === "neuro" ? <NeuroField view={route.view} onOpenCalibration={() => navigate({ id: "mindbody", view: "neuro/calibration" })} onCloseCalibration={() => navigate({ id: "mindbody", view: "neuro" })} training={training} data={data} onRetry={retrySnapshot} onOpenHistory={() => setActive("sessions")} onOpenRecording={(id) => navigate({ id: "mindbody", view: `neuro-history/${id}` })} scanEmpty={scanEmpty} lastSavedId={lastSavedId} onDismissSaved={dismissSaved} focusPrimary={neuroFocus} devices={museDevices} operation={museOperation} error={museError} sessionId={sessionId} sessionStartedAt={sessionStartedAt} sessionEndedAt={sessionEndedAt} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onStart={() => { void startMuseSession(); }} onStop={() => { void stopMuseSession(); }} />
    : screenId === "sessions" ? <SessionsField data={data} onRetry={retrySnapshot} onStartSession={() => { setNeuroFocus((n) => n + 1); setActive("neuro"); }} initialOpenId={historyIdOf(route.view)} onClose={() => navigate({ id: "mindbody", view: "neuro-history" })} />
    : screenId === "business" ? <BusinessField data={data} onRetry={retrySnapshot} view={route.view} onView={(view) => navigate({ id: "enterprise", view })} onOpen={navigate} />
    : screenId === "revenue" ? <RevenueField data={data} onRetry={retrySnapshot} onOpen={navigate} />
    : screenId === "life" ? <LifeField data={data} view={route.view} onOpen={navigate} onRetry={retrySnapshot} />
    : screenId === "assistant" ? <ActivityField data={data} onRetry={retrySnapshot} />
    : screenId === "lists" ? <ListsField refreshSignal={data.refreshedAt} />
    : screenId === "crews" ? <CrewsField data={data} onRetry={retrySnapshot} openRunId={crewRunOf(route)} />
    : screenId === "voice" ? <VoiceField />
    : screenId === "chat" ? <><AgentsField data={data} onOpenCrews={() => navigate({ id: "chat", view: "crews" })} onOpen={navigate} onRetry={retrySnapshot} /><PageSection id="activity"><ActivityField data={data} onRetry={retrySnapshot} onOpen={navigate} /></PageSection></>
    : <SettingsField view={route.view} onRunSetup={() => setSetup("again")} onOpenSmartHome={() => navigate({ id: "smarthome", view: "main" })} data={data} theme={theme} setTheme={setTheme} reduceMotion={reduceMotion} setReduceMotion={setReduceMotion} devices={museDevices} operation={museOperation} error={museError} onScan={() => { void scanForMuse(); }} onConnect={(device) => { void connectToMuse(device); }} onDisconnect={() => { void disconnectFromMuse(); }} onRetry={retrySnapshot} />;

  return (
    <NavyRoot>
    <CommandSurface mode={screenId}>
      {!onLanding && <Toolbar label={fieldLabel} busy={busy} onRefresh={manualRefresh} onSearch={() => setPaletteOpen(true)} />}
      <Splash loaded={data.refreshedAt !== INITIAL_SNAPSHOT.refreshedAt} />
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} onNavigate={navigate}
        onAsk={(text) => { setAskContext(null); setAskDraft((d) => ({ text, n: d.n + 1 })); navigate({ id: "terrance", view: "main" }); }} />
      {!onLanding && <Sidebar active={active} onSelect={setActive} data={data} onOpenServices={() => navigate({ id: "settings", view: "services" })} />}
      <section className="active-field" aria-label={`${fieldLabel} system`}>{subViews && <div className="subview-bar" data-host={active}><SegmentedControl label={`${selected.label} view`} size="small" value={active === "settings" ? settingsTab(route.view) : tabOf(route)} options={subViews.map((entry) => ({ value: entry.view, label: entry.label }))} onChange={(view) => { if (active === "enterprise") writeBusinessView(view); navigate({ id: active, view }); }} /></div>}<ScreenErrorBoundary resetKey={screenId} onRetry={retryLazyScreens}><Suspense fallback={<ScreenSkeleton />}>{view}</Suspense></ScreenErrorBoundary></section>
      {!onLanding && <Footer data={data} busy={busy} />}
      {!onLanding && <AmaraVoiceAnnouncer />}
      {notice && <div className="field-notice"><i />{notice}{noticeLink && notice === "Refreshed" && <button type="button" className="take__ask" onClick={() => { navigate(noticeLink.route); setNotice(null); setNoticeLink(null); }}>{noticeLink.label}</button>}</div>}
      {setup && <FirstRun adapter={standaloneAdapter()} again={setup === "again"} onClose={() => setSetup(null)} />}
      <ContextInspector visible={contextInspectorOpen} onClose={() => setContextInspectorOpen(false)} onChanged={refresh} />
    </CommandSurface>
    </NavyRoot>
  );
}
