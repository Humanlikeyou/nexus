import { InsetGroup, ListRow, SegmentedControl, Toggle } from "../ui";
import { setSandSettings, useSandSettings } from "./settings";

/** Settings › Home lens (design-1118b): sand or the classic lens, Calm, canopy light, and optional sound. */
export function SandSettingsGroup() {
  const s = useSandSettings();
  const sand = s.lens === "sand";
  return (
    <InsetGroup title="Home lens" footer="Sand floats on the home background and follows Amara's voice. Classic keeps the old lens. Canopy light is a soft layer over Home. Sound is off unless you turn it on.">
      <ListRow title="Lens" accessory={<SegmentedControl label="Lens" size="small" value={s.lens} onChange={(v) => setSandSettings({ lens: v as "sand" | "classic" })} options={[{ value: "sand", label: "Sand" }, { value: "classic", label: "Classic lens" }]} />} />
      {sand && <ListRow title="Material" accessory={<SegmentedControl label="Material" size="small" value={s.material} onChange={(v) => setSandSettings({ material: v as "sand" | "fireflies" })} options={[{ value: "sand", label: "Sand" }, { value: "fireflies", label: "Fireflies" }]} />} />}
      <ListRow title="Calm" subtitle="Grains settle without motion. Also follows Reduce motion." accessory={<Toggle aria-label="Calm" checked={s.calm} onCheckedChange={(v) => setSandSettings({ calm: v })} />} />
      <ListRow title="Canopy light" subtitle="Soft sunlight through leaves. Follows the time of day. Still when Calm is on." accessory={<Toggle aria-label="Canopy light" checked={s.tokens} onCheckedChange={(v) => setSandSettings({ tokens: v })} />} />
      <ListRow title="Sound" subtitle="Soft wood and water tones. Off by default." accessory={<Toggle aria-label="Sound" checked={s.sound} onCheckedChange={(v) => setSandSettings({ sound: v })} />} />
      {s.sound && <ListRow title="Volume" accessory={<input type="range" className="sand-volume" min={0} max={100} value={Math.round(s.volume * 100)} aria-label="Sound volume" onChange={(e) => setSandSettings({ volume: Number(e.target.value) / 100 })} />} />}
      {s.sound && <ListRow title="Mute" accessory={<Toggle aria-label="Mute sound" checked={s.mute} onCheckedChange={(v) => setSandSettings({ mute: v })} />} />}
    </InsetGroup>
  );
}
