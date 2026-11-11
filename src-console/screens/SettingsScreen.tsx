import { useSyncExternalStore } from "react";
import { liveVersion, subscribeLive } from "../standalone/liveAdapter";
import { Info, RefreshCw } from "lucide-react";
import type { ThemePreference } from "../design/useAppearance";
import { APP_NAME, APP_VERSION } from "../app/version";
import { dateTime, humanize } from "../lib/format";
import { AllowClaudeGroup } from "../settings/AllowClaude";
import { HomeAssistantSettingsGroup } from "../settings/HomeAssistantSettingsGroup";
import { NoopSettingsGroup } from "../settings/NoopSettingsGroup";
import type { MuseDevice, SystemSnapshot } from "../types";
import { Button, InsetGroup, ListRow, More, PageHero, SegmentedControl, Toggle, Wordmark } from "../ui";
import { DevicesGroup } from "./DevicesScreen";
import type { MuseOperation } from "./muse";
import { nextIn, nextRuns, useSchedulerStatus } from "./TerranceScreen";
import { INITIAL_SNAPSHOT } from "../bridge";
import { useNow } from "../shell/useNow";
import { standaloneAdapter, type StandaloneAdapter } from "../standalone/adapters";
import { ServicesView } from "../standalone/ServicesView";
import { NotificationsView } from "../standalone/NotificationsView";
import { BackupView } from "../standalone/BackupView";
import { UpdatesSection } from "../standalone/UpdatesSection";
import { WhoopPanel } from "../standalone/WhoopPanel";
import { VoiceField } from "./VoiceScreen";
import { MemorySection } from "../personal/PageViews";
import { personalAdapter } from "../personal/adapters";
import "../ui/PanelKit.css";
import "../standalone/standalone.css";
import "./SettingsScreen.css";
import { settingsTab } from "../app/settingsTabs";
export { SETTINGS_TABS, settingsTab } from "../app/settingsTabs";

export type SettingsFieldProps = {
  data: SystemSnapshot;
  theme: ThemePreference;
  setTheme: (theme: ThemePreference) => void;
  reduceMotion: boolean;
  setReduceMotion: (value: boolean) => void;
  devices?: MuseDevice[];
  operation?: MuseOperation;
  error?: string | null;
  onScan?: () => void;
  onConnect?: (device: MuseDevice) => void;
  onDisconnect?: () => void;
  onRetry?: () => void;
  /** design-1111s: Settings tab (General · Services · Devices · Notifications · Backup · Voice · Memory). */
  view?: string;
  onRunSetup?: () => void;
  adapter?: StandaloneAdapter;
};

const noop = () => {};

/** About header (Shell › Wordmark: 32px-cap drawn AMARA, "Version {v}" 12px below). Accessible name "Amara OS". */
export function AboutHeader() {
  return (
    <div className="about" id="about">
      <Wordmark size="about" label={APP_NAME} />
      <p className="about__version">{APP_NAME} · Version {APP_VERSION}</p>
    </div>
  );
}

/** Settings (screen-specs §11, design-1111s tabs). Changes apply immediately. */
export function SettingsField(props: SettingsFieldProps) {
  const { data, devices = [], operation = null, error = null, onScan = noop, onConnect = noop, onDisconnect = noop, onRetry, view, adapter = standaloneAdapter() } = props;
  useSyncExternalStore(subscribeLive, liveVersion); // re-render when the live secrets / key file status loads
  const tab = settingsTab(view);
  if (tab === "services") return <div className="page-inset cc-page settings-tab" data-tab="services"><ServicesView adapter={adapter} /></div>;
  if (tab === "notifications") return <div className="page-inset cc-page settings-tab" data-tab="notifications"><NotificationsView adapter={adapter} /></div>;
  if (tab === "backup") return <div className="page-inset cc-page settings-tab" data-tab="backup" id="backup"><BackupView adapter={adapter} /></div>;
  if (tab === "voice") return <div className="settings-tab settings-tab--plain" data-tab="voice" id="voice"><VoiceField /></div>;
  if (tab === "memory") return <div className="page-inset cc-page settings-tab" data-tab="memory" id="memory"><MemorySection adapter={personalAdapter()} /></div>;
  if (tab === "devices") return (
    <div className="page-inset cc-page settings-tab pk-host sa-page" data-tab="devices">
      <WhoopPanel adapter={adapter} data={data} />
      <DevicesGroup data={data} devices={devices} operation={operation} error={error} onScan={onScan} onConnect={onConnect} onDisconnect={onDisconnect} onRetry={onRetry} />
      <HomeAssistantSettingsGroup />
      <NoopSettingsGroup />
    </div>
  );
  return <GeneralTab {...props} adapter={adapter} />;
}

/** Settings › General: appearance, updates, startup, then diagnostics one step away. */
function GeneralTab({ data, theme, setTheme, reduceMotion, setReduceMotion, onRetry, onRunSetup, adapter }: SettingsFieldProps & { adapter: StandaloneAdapter }) {
  const scheduler = useSchedulerStatus();
  const now = useNow(30_000).getTime();
  const next = nextRuns(scheduler).consolidation;
  const loaded = data.refreshedAt !== INITIAL_SNAPSHOT.refreshedAt;
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const diag = (value: string, command?: string) => (!loaded ? "—" : command && down(command) ? "Unavailable" : value);
  const h = data.hermes;
  return (
    <div className="settings">
      <PageHero title="Settings" statement={!loaded || data.unavailableSources.length === 0 ? "All running" : `${data.unavailableSources.length} source${data.unavailableSources.length === 1 ? "" : "s"} unavailable`} />

      <InsetGroup title="Appearance">
        <ListRow title="Theme" accessory={<SegmentedControl label="Appearance" size="small" value={theme} onChange={setTheme} options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />} />
        <ListRow title="Reduce motion" subtitle="Minimizes transitions and live animation." accessory={<Toggle aria-label="Reduce motion" checked={reduceMotion} onCheckedChange={setReduceMotion} />} />
      </InsetGroup>

      <UpdatesSection adapter={adapter} />

      <InsetGroup title="Startup" footer="Run setup again after moving to a bigger Mac to pick a larger model.">
        <ListRow title="Set up Amara again" subtitle="Restore, model size, downloads and preferences, step by step." accessory={<Button variant="secondary" size="small" onClick={onRunSetup} disabled={!onRunSetup}>Run Setup Again…</Button>} />
      </InsetGroup>

      <More label="Connections and diagnostics" id="settings-more">
      <InsetGroup title="General">
        <ListRow title="Refresh" value="Every 15 seconds" />
        <ListRow title="Data" value="Stored on this Mac" />
        <ListRow title="Background jobs" value={scheduler.running ? "Running" : "Stopped"}
          subtitle={scheduler.running && next != null ? `Next consolidation ${nextIn(next, now)}` : undefined} />
      </InsetGroup>

      <AllowClaudeGroup />

      <AboutHeader />
      <InsetGroup title="About & Diagnostics" footer="For troubleshooting.">
        <ListRow icon={Info} title="Application" value={`${APP_NAME} ${APP_VERSION}`} />
        <ListRow title="Assistant" subtitle={loaded ? `Hermes Agent · ${h.runtime} · ${h.model}` : undefined} value="Amara" />
        <ListRow title="Gateway" value={diag(h.gatewayRunning ? "Running" : "Offline", "get_terrence_runtime_health")} />
        <ListRow title="Muse" value={diag(data.muse.streaming ? "Streaming" : data.muse.connected ? "Connected" : "Disconnected", "get_muse_state")} />
        <ListRow title="Health sync" value={diag(humanize(data.noop.status), "get_noop_health_snapshot")} />
        <ListRow title="Unavailable sources" value={diag(data.unavailableSources.length ? data.unavailableSources.map((s) => s.label).join(", ") : "None")} />
        <ListRow title="Last refresh" value={diag(dateTime(data.refreshedAt))} />
      </InsetGroup>
      {loaded && data.unavailableSources.length > 0 && onRetry && (
        <div><Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button></div>
      )}
      </More>
    </div>
  );
}
