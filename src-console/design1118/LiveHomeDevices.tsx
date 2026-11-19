import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import type { HaDevice_Serialize as Dev, HaDeviceSetResult_Serialize as SetResult, HaDevicesResult_Serialize as Result } from "../bindings.gen";
import { invokeBound } from "../bridge";
import { Button } from "../ui";
import "./design1118.css";

/**
 * Smart home, live devices (decision 0039): names, types and rooms from `ha_devices_list`; controls through
 * `ha_device_set`. Scenes and covers ask for approval in a native dialog on this Mac (the backend shows it;
 * this panel only says so). Scenes can't be undone; a blind's Undo sends the reverse move, which asks again. Status-only devices show their state and no control.
 * Sample controls (SampleHomeControls) stay separate, labelled, and never send.
 */
export type HomeCalls = {
  list: () => Promise<Result>;
  set: (entityId: string, service: string, value: number | null, via: string) => Promise<SetResult>;
  undo: (id: string) => Promise<boolean>;
};
const defaultCalls: HomeCalls = {
  list: () => invokeBound<Result>("ha_devices_list"),
  set: (entityId, service, value, via) => invokeBound<SetResult>("ha_device_set", { entityId, service, value, via }),
  undo: (id) => invokeBound<boolean>("add_undo", { kind: "ha", id }),
};

const TYPE_LABEL: Record<string, string> = { light: "Light", switch: "Switch", fan: "Fan", climate: "Climate", cover: "Cover", scene: "Scene", media: "Media", sensor: "Sensor" };
const APPROVAL_TYPES = new Set(["scene", "cover"]);

/** The service a one-tap control sends, or null when this panel has no control for the device. */
export function controlFor(d: Dev): { on: string; off: string | null } | null {
  if (!d.controllable || d.services.length === 0) return null;
  const has = (s: string) => d.services.includes(s);
  if (d.deviceType === "scene") return has("turn_on") ? { on: "turn_on", off: null } : null;
  if (d.deviceType === "cover") return has("open_cover") ? { on: "open_cover", off: has("close_cover") ? "close_cover" : null } : null;
  if (["light", "switch", "fan"].includes(d.deviceType)) return has("turn_on") ? { on: "turn_on", off: has("turn_off") ? "turn_off" : null } : null;
  return null;
}

const LEVEL_WORD: Record<string, string> = { turn_on: "brightness", set_percentage: "speed", set_cover_position: "position", volume_set: "volume" };
const COMMIT_DELAY_MS = 250;
const COMMIT_KEYS = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"]);

/** The level slider a device accepts (the backend lists `levelService` only when the device reports support), or null. */
export function sliderFor(d: Dev): { service: string; word: string; min: number; scale: number } | null {
  if (!d.controllable || !d.levelService || !d.services.includes(d.levelService)) return null;
  const word = LEVEL_WORD[d.levelService]; if (!word) return null;
  return { service: d.levelService, word, min: d.levelService === "turn_on" ? 1 : 0, scale: d.levelService === "volume_set" ? 100 : 1 };
}

/** Native range input. Moving only changes the local value; release (pointer up or a key) commits once after a short debounce. */
function LevelSlider({ d, busy, onCommit }: { d: Dev; busy: boolean; onCommit: (d: Dev, service: string, value: number) => void }) {
  const cfg = sliderFor(d);
  const current = cfg && d.level != null ? Math.round(d.level * cfg.scale) : null;
  const [val, setVal] = useState<number>(current ?? cfg?.min ?? 0);
  const timer = useRef<number | null>(null);
  const latest = useRef(val);
  const [seen, setSeen] = useState(current);
  if (seen !== current) { setSeen(current); if (current != null) setVal(current); }
  useEffect(() => { if (current != null) latest.current = current; }, [current]);
  useEffect(() => () => { if (timer.current != null) window.clearTimeout(timer.current); }, []);
  const commit = () => {
    if (!cfg) return;
    if (timer.current != null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      const v = latest.current;
      if (v === current) return;
      onCommit(d, cfg.service, v / cfg.scale);
    }, COMMIT_DELAY_MS);
  };
  // The native `change` fires once on release, even when the pointer is let go outside the input
  // (React's onChange follows every `input` while dragging). The debounce folds it with pointerup / keyup.
  const inputRef = useRef<HTMLInputElement>(null);
  const commitRef = useRef(commit);
  useEffect(() => { commitRef.current = commit; });
  const hasSlider = cfg != null;
  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    const onRelease = () => commitRef.current();
    el.addEventListener("change", onRelease);
    return () => el.removeEventListener("change", onRelease);
  }, [hasSlider]);
  if (!cfg) return null;
  return (
    <label className="d18-slider">
      <span>{cfg.word.charAt(0).toUpperCase() + cfg.word.slice(1)}</span>
      <input ref={inputRef} type="range" min={cfg.min} max={100} step={1} value={val} disabled={busy}
        aria-label={`${d.name} ${cfg.word}`} aria-valuetext={`${val}%`}
        onChange={(e) => { const v = Number(e.target.value); latest.current = v; setVal(v); }}
        onPointerUp={commit}
        onKeyUp={(e) => { if (COMMIT_KEYS.has(e.key)) commit(); }} />
    </label>
  );
}

/** A blind's reverse move: back to the position it had, or close after open (and open after close). None after stop. */
export function coverBack(d: Dev, service: string): { d: Dev; service: string; value: number | null } | undefined {
  if (d.deviceType !== "cover") return undefined;
  if (service === "set_cover_position") return d.level != null ? { d, service, value: Math.round(d.level) } : undefined;
  if (service === "open_cover") return { d, service: "close_cover", value: null };
  if (service === "close_cover") return { d, service: "open_cover", value: null };
  return undefined;
}

export function LiveHomeDevices({ calls = defaultCalls }: { calls?: HomeCalls }) {
  const [res, setRes] = useState<Result | null | "error">(null);
  // undo: a History id (Undo via add_undo), or for a blind the reverse move, sent again through ha_device_set
  // so the Mac asks for approval again (Taisun 2026-11-19: "yes add undo").
  const [note, setNote] = useState<{ text: string; undo: string | null; back?: { d: Dev; service: string; value: number | null } } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const load = useCallback(() => { void calls.list().then(setRes).catch(() => setRes("error")); }, [calls]);
  useEffect(load, [load]);
  useEffect(() => { if (!note?.undo && !note?.back) return; const t = window.setTimeout(() => setNote((n) => (n ? { ...n, undo: null, back: undefined } : n)), 8000); return () => window.clearTimeout(t); }, [note]);
  if (res === null || res === "error") return res === "error" ? <p className="d18-note" role="status">Devices couldn&apos;t be read just now.</p> : null;
  const devices = res.devices.filter((d) => !d.system);
  if (devices.length === 0) return null;
  const send = async (d: Dev, service: string, value: number | null = null) => {
    setBusy(d.entityId);
    try {
      const r = await calls.set(d.entityId, service, value, "typed");
      setNote({ text: r.message, undo: r.undoable ? r.id : null, back: r.undoable ? undefined : coverBack(d, service) });
      load();
    } catch { setNote({ text: `${d.name} didn't change. If an approval dialog was open, it may have been declined.`, undo: null }); }
    setBusy(null);
  };
  const undo = async () => {
    const back = note?.back;
    if (back) { setNote({ text: note?.text ?? "", undo: null }); await send(back.d, back.service, back.value); return; }
    const id = note?.undo; if (!id) return;
    setNote({ text: note?.text ?? "", undo: null });
    const ok = await calls.undo(id).catch(() => false);
    setNote({ text: ok ? "Undone." : "That couldn't be undone. Check History.", undo: null });
    load();
  };
  const rooms = new Map<string, Dev[]>();
  for (const d of devices) { const k = res.roomsAvailable ? d.room ?? "No room" : "Devices"; rooms.set(k, [...(rooms.get(k) ?? []), d]); }
  return (
    <section className="d18-card" aria-labelledby="sh-live" data-testid="live-devices">
      <div className="d18-map__head"><h2 id="sh-live" className="d18-h">Your devices</h2></div>
      {[...rooms].map(([room, list]) => (
        <div key={room}>
          <p className="d18-eyebrow">{room}</p>
          {list.map((d) => {
            const c = controlFor(d);
            const asks = APPROVAL_TYPES.has(d.deviceType);
            const on = d.state === "on" || d.state === "open";
            return (
              <Fragment key={d.entityId}>
              <div className="d18-ctl">
                <div><b>{d.name}</b><small>{TYPE_LABEL[d.deviceType] ?? d.deviceType}{d.state ? ` · ${d.state}${d.unit ? ` ${d.unit}` : ""}` : ""}{!c && d.deviceType !== "climate" ? " · status only" : ""}</small>
                  {c && asks && <small>Asks for your approval on this Mac. {d.deviceType === "cover" ? "Undo asks again." : "Can't be undone."}</small>}</div>
                {c && (
                  <span className="d18-actions">
                    <Button size="small" variant="secondary" disabled={busy === d.entityId} aria-label={`${d.deviceType === "scene" ? "Activate" : on && c.off ? "Turn off or close" : "Turn on or open"} ${d.name}`}
                      onClick={() => { void send(d, on && c.off ? c.off : c.on); }}>
                      {d.deviceType === "scene" ? "Activate" : on && c.off ? (d.deviceType === "cover" ? "Close" : "Turn off") : (d.deviceType === "cover" ? "Open" : "Turn on")}
                    </Button>
                  </span>
                )}
              </div>
              {sliderFor(d) && <LevelSlider d={d} busy={busy === d.entityId} onCommit={(dev, svc, v) => { void send(dev, svc, v); }} />}
              </Fragment>
            );
          })}
        </div>
      ))}
      {note && <p className="d18-msg" role="status">{note.text}{(note.undo || note.back) && <> <Button size="small" variant="plain" onClick={() => { void undo(); }}>Undo</Button></>}</p>}
    </section>
  );
}
