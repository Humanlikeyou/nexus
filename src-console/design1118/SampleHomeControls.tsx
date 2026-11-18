import { useState } from "react";
import { DataTag, Toggle } from "../ui";
import "./design1118.css";

/**
 * design-1118a Smart home preview: example controls (sample names) so the deck can be judged before real
 * names and types arrive. Local only: nothing is sent to Home Assistant. Always labelled Sample data.
 */
export function SampleHomeControls() {
  const [scene, setScene] = useState("Evening");
  const [lights, setLights] = useState({ living: true, bedroom: false });
  const [level, setLevel] = useState(70);
  const [temp, setTemp] = useState(21);
  return (
    <>
      <section className="d18-card" aria-labelledby="sh-scenes">
        <div className="d18-map__head"><h2 id="sh-scenes" className="d18-h">Scenes</h2><DataTag kind="sample" /></div>
        <div className="d18-scenes" role="group" aria-label="Scenes">
          {["Evening", "Away", "Night"].map((s) => <button key={s} type="button" className="d18-scene" aria-pressed={scene === s} onClick={() => setScene(s)}>{s}</button>)}
        </div>
      </section>
      <div className="d18-grid2">
        <section className="d18-card" aria-labelledby="sh-lights">
          <div className="d18-map__head"><h2 id="sh-lights" className="d18-h">Lights</h2><DataTag kind="sample" /></div>
          <div className="d18-ctl"><div><b>Living light</b><small>Example device · {lights.living ? `${level}%` : "off"}</small></div><Toggle checked={lights.living} aria-label="Living light" onCheckedChange={(on) => setLights((l) => ({ ...l, living: on }))} /></div>
          {lights.living && <label className="d18-slider"><span>Brightness</span><input type="range" min={1} max={100} value={level} onChange={(e) => setLevel(Number(e.target.value))} aria-label="Living light brightness" /></label>}
          <div className="d18-ctl"><div><b>Bedroom light</b><small>Example device</small></div><Toggle checked={lights.bedroom} aria-label="Bedroom light" onCheckedChange={(on) => setLights((l) => ({ ...l, bedroom: on }))} /></div>
        </section>
        <section className="d18-card" aria-labelledby="sh-climate">
          <div className="d18-map__head"><h2 id="sh-climate" className="d18-h">Climate</h2><DataTag kind="sample" /></div>
          <div className="d18-ctl"><div><b>Main climate</b><small>Example device · {temp}°</small></div></div>
          <label className="d18-slider"><span>Target</span><input type="range" min={16} max={28} step={0.5} value={temp} onChange={(e) => setTemp(Number(e.target.value))} aria-label="Main climate target temperature" /></label>
        </section>
      </div>
    </>
  );
}
