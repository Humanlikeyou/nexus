import { Bluetooth, ChevronRight, HeartPulse, Moon, Plus, Search, Settings2, Sun, SunMoon, Trash2, Wifi } from "lucide-react";
import { useState } from "react";
import { Badge, Button, Card, InsetGroup, ListRow, SegmentedControl, StatTile, StatusDot, TextField, Toggle, type Tone } from "../index";

const TONES: Tone[] = ["neutral", "accent", "info", "success", "warning", "danger"];

/** Every ui/ primitive in its main states. Rendered once per theme by gallery/main.tsx. */
export function Gallery({ theme }: { theme: "light" | "dark" }) {
  const [range, setRange] = useState<"day" | "week" | "month">("week");
  const [appearance, setAppearance] = useState<"system" | "light" | "dark">(theme);
  const [sync, setSync] = useState(true);
  const [motion, setMotion] = useState(false);
  const [source, setSource] = useState("muse");
  const [loading, setLoading] = useState(true);

  return (
    <main className="ui-gallery">
      <header className="ui-gallery__header">
        <h1>ui/ primitives</h1>
        <Badge tone="accent">{theme}</Badge>
      </header>

      <Card title="Button" accessory={<Toggle checked={loading} onCheckedChange={setLoading}>Loading</Toggle>}>
        <div className="ui-gallery__row">
          <Button variant="primary" icon={Plus}>New session</Button>
          <Button>Secondary</Button>
          <Button variant="plain">Plain</Button>
          <Button variant="destructive" icon={Trash2}>Delete</Button>
          <Button variant="primary" loading={loading}>Saving changes</Button>
          <Button iconOnly icon={Settings2} aria-label="Settings" />
        </div>
        <div className="ui-gallery__row">
          <Button size="small" variant="primary">Small primary</Button>
          <Button size="small">Small</Button>
          <Button size="small" variant="plain">Small plain</Button>
          <Button size="small" iconOnly icon={Plus} aria-label="Add" />
          <Button disabled>Disabled</Button>
        </div>
      </Card>

      <div className="ui-gallery__grid">
        <Card title="TextField">
          <TextField label="Session name" placeholder="Morning focus" description="Shown in Sessions and exports." />
          <TextField label="Search" hideLabel leadingIcon={Search} placeholder="Search" />
          <TextField label="Gateway port" defaultValue="18789x" error="Use digits only (1024–65535)." />
        </Card>

        <Card title="SegmentedControl and Toggle">
          <SegmentedControl label="Range" value={range} onChange={setRange} options={[{ value: "day", label: "Day" }, { value: "week", label: "Week" }, { value: "month", label: "Month" }]} />
          <SegmentedControl label="Appearance" size="small" value={appearance} onChange={setAppearance} options={[{ value: "system", label: "System", icon: SunMoon }, { value: "light", label: "Light", icon: Sun }, { value: "dark", label: "Dark", icon: Moon }]} />
          <div className="ui-gallery__stack">
            <Toggle checked={sync} onCheckedChange={setSync}>Sync with NOOP</Toggle>
            <Toggle checked={motion} onCheckedChange={setMotion}>Reduce motion</Toggle>
            <Toggle checked={false} onCheckedChange={() => {}} disabled>Disabled</Toggle>
          </div>
        </Card>
      </div>

      <div className="ui-gallery__grid">
        <InsetGroup title="Sources" footer="Selectable rows are buttons; the selected row has aria-current.">
          <ListRow icon={Bluetooth} title="Muse S" subtitle="Connected · 82% battery" value="EEG" onSelect={() => setSource("muse")} selected={source === "muse"} accessory={<ChevronRight aria-hidden="true" />} />
          <ListRow icon={HeartPulse} title="NOOP" subtitle="Read-only health store" value="HRV" onSelect={() => setSource("noop")} selected={source === "noop"} accessory={<ChevronRight aria-hidden="true" />} />
          <ListRow icon={Wifi} title="OpenClaw gateway" subtitle="127.0.0.1:18789" accessory={<StatusDot tone="success">Online</StatusDot>} />
          <ListRow title="Background sync" accessory={<Toggle aria-label="Background sync" checked={sync} onCheckedChange={setSync} />} />
        </InsetGroup>

        <Card title="Badge and StatusDot" elevation={0}>
          <div className="ui-gallery__row">{TONES.map((tone) => <Badge key={tone} tone={tone}>{tone}</Badge>)}</div>
          <div className="ui-gallery__row">
            <StatusDot tone="success" pulse>Streaming</StatusDot>
            <StatusDot tone="warning">Degraded</StatusDot>
            <StatusDot tone="danger">Offline</StatusDot>
            <StatusDot>Idle</StatusDot>
          </div>
        </Card>
      </div>

      <div className="ui-gallery__stats">
        <StatTile label="Recovery" value={74} unit="%" trend={{ direction: "up", value: "+6%", label: "Up 6% from yesterday", sentiment: "positive" }} />
        <StatTile label="HRV" value={62} unit="ms" trend={{ direction: "down", value: "−4 ms", label: "Down 4 ms from yesterday", sentiment: "negative" }} />
        <StatTile label="Strain" value="10.2" trend={{ direction: "flat", value: "0.0", label: "No change from yesterday" }} />
        <StatTile label="Focus sessions" value={128} />
      </div>

      <div className="ui-gallery__grid">
        <Card title="Elevation 2" elevation={2}><p className="ui-gallery__note">Raised surface for popovers and key panels.</p></Card>
        <Card padding="none" title="Padding none"><p className="ui-gallery__note ui-gallery__note--padded">Content handles its own inset.</p></Card>
      </div>
    </main>
  );
}
