import { Info, RefreshCw } from "lucide-react";
import type { ThemePreference } from "../design/useAppearance";
import { APP_NAME, APP_VERSION } from "../app/version";
import { dateTime, humanize } from "../lib/format";
import { BackupSection } from "../settings/BackupSection";
import type { MuseDevice, SystemSnapshot } from "../types";
import { Button, InsetGroup, ListRow, SegmentedControl, Toggle, Wordmark } from "../ui";
import { DevicesGroup } from "./DevicesScreen";
import type { MuseOperation } from "./muse";
import { nextIn, nextRuns, useSchedulerStatus } from "./TerranceScreen";
import { INITIAL_SNAPSHOT } from "../bridge";
import { useNow } from "../shell/useNow";
import "./SettingsScreen.css";

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

/** Settings (screen-specs §11): one 640px column of System Settings style groups. Changes apply immediately. */
export function SettingsField({ data, theme, setTheme, reduceMotion, setReduceMotion, devices = [], operation = null, error = null, onScan = noop, onConnect = noop, onDisconnect = noop, onRetry }: SettingsFieldProps) {
  const scheduler = useSchedulerStatus();
  const now = useNow(30_000).getTime();
  const next = nextRuns(scheduler).consolidation;
  const loaded = data.refreshedAt !== INITIAL_SNAPSHOT.refreshedAt;
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const diag = (value: string, command?: string) => (!loaded ? "—" : command && down(command) ? "Unavailable" : value);
  const h = data.hermes;
  return (
    <div className="settings">
      <h1 className="settings__title">Settings</h1>

      <InsetGroup title="Appearance">
        <ListRow title="Appearance" accessory={<SegmentedControl label="Appearance" size="small" value={theme} onChange={setTheme} options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />} />
        <ListRow title="Reduce motion" subtitle="Minimizes transitions and live animation." accessory={<Toggle aria-label="Reduce motion" checked={reduceMotion} onCheckedChange={setReduceMotion} />} />
      </InsetGroup>

      <DevicesGroup data={data} devices={devices} operation={operation} error={error} onScan={onScan} onConnect={onConnect} onDisconnect={onDisconnect} onRetry={onRetry} />

      <InsetGroup title="General">
        <ListRow title="Refresh" value="Every 15 seconds" />
        <ListRow title="Data" value="Stored on this Mac" />
        <ListRow title="Background jobs" value={scheduler.running ? "Running" : "Stopped"}
          subtitle={scheduler.running && next != null ? `Next consolidation ${nextIn(next, now)}` : undefined} />
      </InsetGroup>

      <BackupSection />

      <AboutHeader />
      <InsetGroup title="About & Diagnostics" footer="For troubleshooting.">
        <ListRow icon={Info} title="Application" value={`${APP_NAME} ${APP_VERSION}`} />
        <ListRow title="Assistant" subtitle={loaded ? `Hermes Agent · ${h.runtime} · ${h.model}` : undefined} value="Amara" />
        <ListRow title="Gateway" value={diag(h.gatewayRunning ? "Running" : "Offline", "get_terrence_runtime_health")} />
        <ListRow title="Muse" value={diag(data.muse.streaming ? "Streaming" : data.muse.connected ? "Connected" : "Disconnected", "get_muse_state")} />
        <ListRow title="NOOP" value={diag(humanize(data.noop.status), "get_noop_health_snapshot")} />
        <ListRow title="Unavailable sources" value={diag(data.unavailableSources.length ? data.unavailableSources.map((s) => s.label).join(", ") : "None")} />
        <ListRow title="Last refresh" value={diag(dateTime(data.refreshedAt))} />
      </InsetGroup>
      {loaded && data.unavailableSources.length > 0 && onRetry && (
        <div><Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button></div>
      )}
    </div>
  );
}
