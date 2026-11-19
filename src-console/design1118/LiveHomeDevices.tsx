import { useCallback, useEffect, useState } from "react";
import type { HaDevice_Serialize as Dev, HaDeviceSetResult_Serialize as SetResult, HaDevicesResult_Serialize as Result } from "../bindings.gen";
import { invokeBound } from "../bridge";
import { Button } from "../ui";
import "./design1118.css";

/**
 * Smart home, live devices (decision 0039): names, types and rooms from `ha_devices_list`; controls through
 * `ha_device_set`. Scenes and covers ask for approval in a native dialog on this Mac (the backend shows it;
 * this panel only says so) and can't be undone. Status-only devices show their state and no control.
 * Sample controls (SampleHomeControls) stay separate, labelled, and never send.
 */
export type HomeCalls = {
  list: () => Promise<Result>;
  set: (entityId: string, service: string, value: null, via: string) => Promise<SetResult>;
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

export function LiveHomeDevices({ calls = defaultCalls }: { calls?: HomeCalls }) {
  const [res, setRes] = useState<Result | null | "error">(null);
  const [note, setNote] = useState<{ text: string; undo: string | null } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const load = useCallback(() => { void calls.list().then(setRes).catch(() => setRes("error")); }, [calls]);
  useEffect(load, [load]);
  useEffect(() => { if (!note?.undo) return; const t = window.setTimeout(() => setNote((n) => (n ? { ...n, undo: null } : n)), 8000); return () => window.clearTimeout(t); }, [note]);
  if (res === null || res === "error") return res === "error" ? <p className="d18-note" role="status">Devices couldn&apos;t be read just now.</p> : null;
  const devices = res.devices.filter((d) => !d.system);
  if (devices.length === 0) return null;
  const send = async (d: Dev, service: string) => {
    setBusy(d.entityId);
    try {
      const r = await calls.set(d.entityId, service, null, "typed");
      setNote({ text: r.message, undo: r.undoable ? r.id : null });
      load();
    } catch { setNote({ text: `${d.name} didn't change. If an approval dialog was open, it may have been declined.`, undo: null }); }
    setBusy(null);
  };
  const undo = async () => {
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
              <div className="d18-ctl" key={d.entityId}>
                <div><b>{d.name}</b><small>{TYPE_LABEL[d.deviceType] ?? d.deviceType}{d.state ? ` · ${d.state}${d.unit ? ` ${d.unit}` : ""}` : ""}{!c && d.deviceType !== "climate" ? " · status only" : ""}</small>
                  {c && asks && <small>Asks for your approval on this Mac. Can&apos;t be undone.</small>}</div>
                {c && (
                  <span className="d18-actions">
                    <Button size="small" variant="secondary" disabled={busy === d.entityId} aria-label={`${d.deviceType === "scene" ? "Activate" : on && c.off ? "Turn off or close" : "Turn on or open"} ${d.name}`}
                      onClick={() => { void send(d, on && c.off ? c.off : c.on); }}>
                      {d.deviceType === "scene" ? "Activate" : on && c.off ? (d.deviceType === "cover" ? "Close" : "Turn off") : (d.deviceType === "cover" ? "Open" : "Turn on")}
                    </Button>
                  </span>
                )}
              </div>
            );
          })}
        </div>
      ))}
      {note && <p className="d18-msg" role="status">{note.text}{note.undo && <> <Button size="small" variant="plain" onClick={() => { void undo(); }}>Undo</Button></>}</p>}
    </section>
  );
}
