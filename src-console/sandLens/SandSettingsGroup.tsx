import { InsetGroup, ListRow, SegmentedControl, Toggle } from "../ui";
import { setSandSettings, useSandSettings } from "./settings";

/** Settings › General › Home lens (design-1118b): each Stage B step behind its own toggle. */
export function SandSettingsGroup() {
  const s = useSandSettings();
  const sand = s.lens === "sand";
  return (
    <InsetGroup title="Home lens" footer="Sand sits on the home background and follows Amara's voice. Classic keeps the old lens. Sound is off unless you turn it on.">
      <ListRow title="Lens" accessory={<SegmentedControl label="Lens" size="small" value={s.lens} onChange={(v) => setSandSettings({ lens: v as "sand" | "classic" })} options={[{ value: "sand", label: "Sand" }, { value: "classic", label: "Classic lens" }]} />} />
      {sand && <ListRow title="Material" accessory={<SegmentedControl label="Material" size="small" value={s.material} onChange={(v) => setSandSettings({ material: v as "sand" | "fireflies" })} options={[{ value: "sand", label: "Sand" }, { value: "fireflies", label: "Fireflies" }]} />} />}
      <ListRow title="Calm" subtitle="Grains settle without motion. Also follows Reduce motion." accessory={<Toggle aria-label="Calm" checked={s.calm} onCheckedChange={(v) => setSandSettings({ calm: v })} />} />
      <ListRow title="Light and depth" subtitle="Lit edges and shadows follow the canopy light." accessory={<Toggle aria-label="Light and depth" checked={s.tokens} onCheckedChange={(v) => setSandSettings({ tokens: v })} />} />
      <ListRow title="Ambience from recovery" subtitle="Uses your WHOOP recovery. Neutral when there is none." accessory={<Toggle aria-label="Ambience from recovery" checked={s.ambience} onCheckedChange={(v) => setSandSettings({ ambience: v })} />} />
      <ListRow title="Micro-moments" subtitle="Small sand moments for a finished task or a notice." accessory={<Toggle aria-label="Micro-moments" checked={s.moments} onCheckedChange={(v) => setSandSettings({ moments: v })} />} />
      <ListRow title="Sound" subtitle="Soft wood and water tones. Off by default." accessory={<Toggle aria-label="Sound" checked={s.sound} onCheckedChange={(v) => setSandSettings({ sound: v })} />} />
      {s.sound && <ListRow title="Volume" accessory={<input type="range" className="sand-volume" min={0} max={100} value={Math.round(s.volume * 100)} aria-label="Sound volume" onChange={(e) => setSandSettings({ volume: Number(e.target.value) / 100 })} />} />}
      {s.sound && <ListRow title="Mute" accessory={<Toggle aria-label="Mute sound" checked={s.mute} onCheckedChange={(v) => setSandSettings({ mute: v })} />} />}
    </InsetGroup>
  );
}
