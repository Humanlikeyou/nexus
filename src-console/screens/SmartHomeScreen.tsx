import { Lightbulb, Plug, Power, RefreshCw, Thermometer, ToggleLeft, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  type HaContainerStatus, type HaEntity, type HaInactiveKind, type HaLiveState,
  HA_ERROR_COPY, applyStateChange, classifyHaError, containerErrorCopy, inactiveKindFor, isServiceRefusal, domainLabel, entityName, haBackend, isToggleable, sortDevices, stateLabel, toggleService,
} from "../lib/homeAssistant";
import { Badge, Button, InsetGroup, ListRow, Toggle } from "../ui";
import "./SmartHomeScreen.css";

export type DevicesState =
  | { status: "loading" }
  | { status: "ready"; devices: HaEntity[] }
  | { status: "inactive"; kind: HaInactiveKind }
  | { status: "error" };

type Container = { status: "loading" } | { status: "ok"; value: HaContainerStatus } | { status: "unavailable"; message: string };

const ICONS: Record<string, LucideIcon> = { light: Lightbulb, switch: Plug, fan: Power, input_boolean: ToggleLeft, sensor: Thermometer };

/** Fixed setup copy per inactive kind (Rust's error text is never shown raw). */
export const INACTIVE_COPY: Record<HaInactiveKind, { title: string; intro: string; steps: string[] }> = {
  not_configured: {
    title: "Home Assistant isn't set up",
    intro: "Amara OS reads your home through the local Home Assistant. To set it up:",
    steps: [
      "In ~/projects/amara-voice/.env, set HA_URL to http://127.0.0.1:8123 and add a long-lived HA_TOKEN.",
      "Keep the file private (chmod 600 .env).",
      "Start the container below, then choose Check again.",
    ],
  },
  auth: {
    title: "Home Assistant rejected the token",
    intro: "The token in ~/projects/amara-voice/.env didn't work. To fix it:",
    steps: [
      "In Home Assistant, open your profile, then Security, and create a long-lived access token.",
      "Replace HA_TOKEN in ~/projects/amara-voice/.env with the new token.",
      "Choose Check again.",
    ],
  },
  unreachable: {
    title: "Home Assistant isn't reachable",
    intro: "Amara OS couldn't reach Home Assistant at 127.0.0.1:8123. To fix it:",
    steps: ["Start the container below.", "Wait about a minute for Home Assistant to finish starting.", "Choose Check again."],
  },
  backend_missing: {
    title: "Home Assistant support isn't in this build",
    intro: "This version of Amara OS doesn't include the Home Assistant bridge yet.",
    steps: ["Update Amara OS, then come back here."],
  },
};

export async function loadDevices(): Promise<DevicesState> {
  try {
    const states = await haBackend().states();
    if (!Array.isArray(states)) return { status: "error" };
    return { status: "ready", devices: sortDevices(states) };
  } catch (error) {
    const kind = classifyHaError(error);
    console.warn(`[ha] states unavailable: ${kind}`);
    const inactive = inactiveKindFor(kind);
    return inactive ? { status: "inactive", kind: inactive } : { status: "error" };
  }
}

const LIVE_TONE: Record<HaLiveState, "success" | "warning" | "neutral" | "danger"> = { connecting: "neutral", connected: "success", authFailed: "danger", disconnected: "warning", stopped: "neutral" };
const LIVE_COPY: Record<HaLiveState, string> = { connecting: "Connecting…", connected: "Live", authFailed: "Token rejected", disconnected: "Reconnecting…", stopped: "Live updates off" };

function ContainerBar({ container, busy, onStart, onStop, error }: { container: Container; busy: "start" | "stop" | null; onStart: () => void; onStop: () => void; error: string | null }) {
  const running = container.status === "ok" && container.value.running;
  const label = container.status === "loading" ? "Checking…" : container.status === "unavailable" ? "Unknown" : running ? "Up" : "Down";
  const tone = container.status === "ok" ? (running ? "success" : "warning") : "neutral";
  return (
    <section className="smart-home__container" aria-labelledby="sh-container" aria-busy={busy !== null || undefined}>
      <div className="smart-home__container-row">
        <h2 id="sh-container" className="smart-home__title">Container</h2>
        <Badge tone={tone} aria-label={`Home Assistant container: ${label}`}>{label}</Badge>
        {container.status === "ok" && container.value.status && <span className="smart-home__meta">{container.value.status}</span>}
        <span className="smart-home__actions">
          <Button size="small" icon={Power} onClick={onStart} loading={busy === "start"} disabled={busy !== null || running || container.status === "loading"}>Start</Button>
          <Button size="small" onClick={onStop} loading={busy === "stop"} disabled={busy !== null || !running}>Stop</Button>
        </span>
      </div>
      {busy && <p className="smart-home__meta" role="status">{busy === "start" ? "Starting Home Assistant… this can take up to 2 minutes." : "Stopping Home Assistant…"}</p>}
      {container.status === "unavailable" && <p className="smart-home__meta">{container.message}</p>}
      {error && <p role="alert" className="smart-home__error">{error}</p>}
    </section>
  );
}

export function Inactive({ kind, onRetry }: { kind: HaInactiveKind; onRetry: () => void }) {
  const copy = INACTIVE_COPY[kind];
  return (
    <div className="subsystem-inactive" role="region" aria-label="Home Assistant setup">
      <h2>{copy.title}</h2>
      <p>{copy.intro}</p>
      <ol>{copy.steps.map((s) => <li key={s}>{s}</li>)}</ol>
      <div><Button size="small" icon={RefreshCw} onClick={onRetry}>Check again</Button></div>
    </div>
  );
}

export function SmartHomeField({ load = loadDevices }: { load?: () => Promise<DevicesState> }) {
  const [state, setState] = useState<DevicesState>({ status: "loading" });
  const [container, setContainer] = useState<Container>({ status: "loading" });
  const [busy, setBusy] = useState<"start" | "stop" | null>(null);
  const [containerError, setContainerError] = useState<string | null>(null);
  const [pending, setPending] = useState<Record<string, boolean>>({});
  const [rowError, setRowError] = useState<{ entityId: string; message: string } | null>(null);
  const [live, setLive] = useState<HaLiveState | null>(null);
  const mounted = useRef(true);

  const refreshContainer = useCallback(() => {
    haBackend().containerStatus().then(
      (value) => { if (mounted.current) setContainer({ status: "ok", value }); },
      (error) => { if (mounted.current) setContainer({ status: "unavailable", message: containerErrorCopy(error) }); },
    );
  }, []);
  const refresh = useCallback(() => {
    setState({ status: "loading" });
    void load().then((s) => { if (mounted.current) setState(s); });
    refreshContainer();
  }, [load, refreshContainer]);

  useEffect(() => {
    mounted.current = true;
    void load().then((s) => { if (mounted.current) setState(s); });
    refreshContainer();
    const ha = haBackend();
    const unlisten: Array<() => void> = [];
    void ha.onStateChanged((change) => { if (mounted.current) setState((s) => s.status === "ready" ? { ...s, devices: applyStateChange(s.devices, change) } : s); })
      .then((u) => { if (mounted.current) unlisten.push(u); else u(); }, () => {});
    void ha.onLiveStatus((s) => { if (mounted.current) setLive(s); })
      .then((u) => { if (mounted.current) unlisten.push(u); else u(); }, () => {});
    ha.liveStart().catch(() => {});
    return () => { mounted.current = false; unlisten.forEach((u) => u()); ha.liveStop().catch(() => {}); };
  }, [load, refreshContainer]);

  // Start/stop run ONLY from the explicit button clicks below: never on mount, in an effect,
  // on reconnect or as an auto-retry. The ref blocks a second click before React re-renders.
  const busyRef = useRef(false);
  const runContainer = async (action: "start" | "stop") => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(action);
    setContainerError(null);
    try {
      const value = action === "start" ? await haBackend().containerStart() : await haBackend().containerStop();
      if (!mounted.current) return;
      setContainer({ status: "ok", value });
      if (action === "start") void load().then((s) => { if (mounted.current) setState(s); });
    } catch (error) {
      if (mounted.current) setContainerError(containerErrorCopy(error));
    } finally {
      busyRef.current = false;
      if (mounted.current) setBusy(null);
    }
  };

  const toggle = async (device: HaEntity, on: boolean) => {
    setRowError(null);
    setPending((p) => ({ ...p, [device.entityId]: true }));
    try {
      await haBackend().callService(device.domain, toggleService(on), device.entityId);
      if (mounted.current) setState((s) => s.status === "ready" ? { ...s, devices: s.devices.map((d) => d.entityId === device.entityId ? { ...d, state: on ? "on" : "off" } : d) } : s);
    } catch (error) {
      // The switch never flipped optimistically, so it stays at its previous state (reverted).
      const kind = classifyHaError(error);
      const message = isServiceRefusal(kind) ? HA_ERROR_COPY[kind] : `Couldn't turn ${on ? "on" : "off"} ${entityName(device)}.`;
      if (mounted.current) setRowError({ entityId: device.entityId, message });
    } finally {
      if (mounted.current) setPending((p) => { const next = { ...p }; delete next[device.entityId]; return next; });
    }
  };

  return (
    <div className="smart-home">
      <header className="smart-home__header">
        <h1>Smart home</h1>
        <p className="smart-home__subtitle">Devices from your local Home Assistant.</p>
        {live && <Badge className="smart-home__live" tone={LIVE_TONE[live]} role="status" aria-label={`Live updates: ${LIVE_COPY[live]}`}>{LIVE_COPY[live]}</Badge>}
      </header>
      <ContainerBar container={container} busy={busy} error={containerError} onStart={() => { void runContainer("start"); }} onStop={() => { void runContainer("stop"); }} />
      {state.status === "loading" ? <p className="smart-home__meta" role="status">Loading devices…</p>
        : state.status === "inactive" ? <Inactive kind={state.kind} onRetry={refresh} />
          : live === "authFailed" ? <Inactive kind="auth" onRetry={refresh} />
          : state.status === "error" ? (
            <div className="smart-home__panel" role="region" aria-label="Devices">
              <p>Couldn't load devices.</p>
              <div><Button size="small" icon={RefreshCw} onClick={refresh}>Retry</Button></div>
            </div>
          ) : state.devices.length === 0 ? (
            <div className="smart-home__panel smart-home__empty" role="region" aria-label="Devices">
              <h2 className="smart-home__title">No devices yet.</h2>
              <p className="smart-home__meta">Add devices in Home Assistant and they'll show up here.</p>
            </div>
          ) : (
            <div className="smart-home__list">
              <InsetGroup title="Devices" footer="Changes come from Home Assistant as they happen.">
                {state.devices.map((d) => (
                  <ListRow key={d.entityId} icon={ICONS[d.domain]} title={entityName(d)}
                    subtitle={rowError?.entityId === d.entityId ? <span role="alert" className="smart-home__row-error">{rowError.message}</span> : domainLabel(d.domain)}
                    value={isToggleable(d) ? undefined : stateLabel(d)}
                    accessory={isToggleable(d) ? <Toggle checked={d.state === "on"} disabled={!!pending[d.entityId]} aria-label={entityName(d)} onCheckedChange={(on) => { void toggle(d, on); }} /> : undefined} />
                ))}
              </InsetGroup>
            </div>
          )}
    </div>
  );
}
