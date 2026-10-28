import { PageHeading, SectionHeading, SegmentedControl, StatusIndicator } from "../design/ProductUI";
import type { ThemePreference } from "../design/useAppearance";
import { date } from "../lib/format";
import { BackupSection } from "../settings/BackupSection";
import type { SystemSnapshot } from "../types";
import { HardDrive, SlidersHorizontal, Sun } from "lucide-react";

export function SettingsField({ data, theme, setTheme, reduceMotion, setReduceMotion }: { data: SystemSnapshot; theme: ThemePreference; setTheme: (theme: ThemePreference) => void; reduceMotion: boolean; setReduceMotion: (value: boolean) => void }) {
  return <div className="system-field settings-field">
    <PageHeading eyebrow="Nexus" title="Settings" description="Appearance and local application behavior." />
    <div className="settings-layout">
      <nav className="settings-index" aria-label="Settings sections"><a href="#appearance"><Sun size={15} />Appearance</a><a href="#general"><SlidersHorizontal size={15} />General</a><a href="#advanced"><HardDrive size={15} />Advanced</a></nav>
      <div className="settings-content">
        <section id="appearance" className="settings-section"><SectionHeading title="Appearance" detail="Choose how Nexus looks on this Mac." /><div className="setting-row"><div><strong>Theme</strong><p>Follow macOS or keep a consistent appearance.</p></div><SegmentedControl value={theme} label="Theme" onChange={setTheme} options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} /></div><label className="setting-row setting-toggle"><div><strong>Reduce motion</strong><p>Minimize interface transitions and live state animation.</p></div><input type="checkbox" checked={reduceMotion} onChange={(event) => setReduceMotion(event.target.checked)} /><span aria-hidden="true" /></label></section>
        <section id="general" className="settings-section"><SectionHeading title="General" detail="Current application behavior." /><div className="setting-row"><div><strong>Automatic refresh</strong><p>Health, action, and runtime status refresh every 15 seconds.</p></div><StatusIndicator tone="success">On</StatusIndicator></div><div className="setting-row"><div><strong>Local-first data</strong><p>Sessions, context, and preferences stay in local Nexus stores.</p></div><StatusIndicator tone="success">Active</StatusIndicator></div></section>
        <section id="advanced" className="settings-section"><SectionHeading title="Advanced" detail="Runtime details for troubleshooting." /><BackupSection /><details className="diagnostics-disclosure" open><summary>Diagnostics</summary><dl><div><dt>Application</dt><dd>Nexus 0.1.0</dd></div><div><dt>Runtime</dt><dd>{data.hermes.runtime} · {data.hermes.model}</dd></div><div><dt>Gateway</dt><dd>{data.hermes.gatewayRunning ? "Running" : "Offline"}</dd></div><div><dt>Muse</dt><dd>{data.muse.streaming ? "Streaming" : data.muse.connected ? "Connected" : "Disconnected"}</dd></div><div><dt>NOOP health</dt><dd>{data.noop.status.replace(/_/g, " ")}</dd></div><div><dt>Last refresh</dt><dd>{date(data.refreshedAt)}</dd></div></dl></details></section>
      </div>
    </div>
  </div>;
}
