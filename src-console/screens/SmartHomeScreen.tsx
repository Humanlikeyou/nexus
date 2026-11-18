import { Lightbulb, Minus, Plug, Plus, Power, RefreshCw, Settings2, Thermometer, ThermometerSun, ToggleLeft, type LucideIcon } from "lucide-react";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import {
  type HaContainerStatus, type HaEntity, type HaInactiveKind, type HaLiveState,
  HA_ERROR_COPY, HVAC_LABEL, type HaServiceData, applyStateChange, rememberEntityNames, clampTemperature, climateRange, hvacModesFor, isHvacMode, targetTemperature, validServiceData, classifyHaError, containerErrorCopy, inactiveKindFor, isServiceRefusal, domainLabel, entityName, haBackend, isToggleable, sortDevices, stateLabel, toggleService,
} from "../lib/homeAssistant";
import { Badge, Button, InsetGroup, ListRow, More, PageHero, Select, Toggle } from "../ui";
import { displayName, groupDevices } from "../design1118/homeGroups";
import { PageTake } from "../design1118/PageTake";
import { SampleHomeControls } from "../design1118/SampleHomeControls";
import { sampleAdaptersEnabled } from "../lib/sampleAdapters";
import "./SmartHomeScreen.css";

export type DevicesState =
  | { status: "loading" }
  | { status: "ready"; devices: HaEntity[] }
  | { status: "inactive"; kind: HaInactiveKind }
  | { status: "error" };

type Container = { status: "loading" } | { status: "ok"; value: HaContainerStatus } | { status: "unavailable"; message: string };

const ICONS: Record<string, LucideIcon> = { light: Lightbulb, switch: Plug, fan: Power, input_boolean: ToggleLeft, sensor: Thermometer, climate: ThermometerSun };

/** Fixed setup copy per inactive kind (Rust's error text is never shown raw). */
export const INACTIVE_COPY: Record<HaInactiveKind, { title: string; intro: string; steps: string[] }> = {
  not_configured: {
    title: "Home Assistant isn't set up",
    intro: "Amara OS reads your home through the local Home Assistant. To set it up:",
    steps: [
      "Open Settings, then Home Assistant, and add the address (usually http://127.0.0.1:8123) and a long-lived access token.",
      "Start the container below, then choose Check again.",
    ],
  },
  auth: {
    title: "Home Assistant rejected the token",
    intro: "The saved access token didn't work. To fix it:",
    steps: [
      "In Home Assistant, open your profile, then Security, and create a long-lived access token.",
      "Open Settings, then Home Assistant, and choose Replace token.",
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

type ClimateSend = (service: "set_temperature" | "set_hvac_mode", data: HaServiceData) => void;

/** Climate setpoint stepper (clamped to the entity's range inside 5–95) and an hvac mode select. */
export function ClimateControl({ device, pending, onSend }: { device: HaEntity; pending: boolean; onSend: ClimateSend }) {
  const name = entityName(device);
  const range = climateRange(device);
  const target = targetTemperature(device);
  const [draft, setDraft] = useState(target == null ? "" : String(target));
  const [invalid, setInvalid] = useState(false);
  // Re-sync to the entity after each call settles (success or failure, so a refusal reverts the field).
  const syncKey = pending ? null : `${target}`;
  const [lastSync, setLastSync] = useState(syncKey);
  if (syncKey !== lastSync) {
    setLastSync(syncKey);
    if (syncKey !== null) { setDraft(target == null ? "" : String(target)); setInvalid(false); }
  }
  const modes = hvacModesFor(device);
  const send = (t: number) => {
    const next = clampTemperature(t, range);
    setDraft(String(next));
    setInvalid(false);
    if (next !== target) onSend("set_temperature", { temperature: next });
  };
  const commitDraft = () => {
    const t = Number(draft);
    if (draft.trim() === "" || !Number.isFinite(t)) { setInvalid(true); return; }
    send(t);
  };
  const base = target ?? clampTemperature((range.min + range.max) / 2, range);
  const unit = typeof device.attributes.temperature_unit === "string" ? device.attributes.temperature_unit : "°";
  return (
    <span className="smart-home__climate" role="group" aria-label={`${name} controls`}>
      <Button size="small" variant="plain" iconOnly icon={Minus} aria-label={`Lower ${name} temperature`} disabled={pending || base <= range.min} onClick={() => send(base - range.step)} />
      <input className="smart-home__temp" type="number" inputMode="decimal" min={range.min} max={range.max} step={range.step}
        aria-label={`${name} target temperature`} aria-invalid={invalid || undefined} value={draft} disabled={pending}
        onChange={(e) => setDraft(e.target.value)} onBlur={commitDraft} onKeyDown={(e) => { if (e.key === "Enter") commitDraft(); }} />
      <span className="smart-home__unit" aria-hidden="true">{unit}</span>
      <Button size="small" variant="plain" iconOnly icon={Plus} aria-label={`Raise ${name} temperature`} disabled={pending || base >= range.max} onClick={() => send(base + range.step)} />
      {modes.length > 0 && (
        <Select label={`${name} mode`} hideLabel className="smart-home__mode" disabled={pending}
          value={isHvacMode(device.state) ? device.state : ""}
          options={[...(isHvacMode(device.state) ? [] : [{ value: "", label: "Mode", disabled: true }]), ...modes.map((m) => ({ value: m, label: HVAC_LABEL[m] }))]}
          onChange={(e) => { if (isHvacMode(e.target.value) && e.target.value !== device.state) onSend("set_hvac_mode", { hvacMode: e.target.value }); }} />
      )}
    </span>
  );
}

export async function loadDevices(): Promise<DevicesState> {
  try {
    const states = await haBackend().states();
    if (!Array.isArray(states)) return { status: "error" };
    rememberEntityNames(states);
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

/** Inactive kinds that are fixed from Settings › Home Assistant. */
const SETTINGS_KINDS: ReadonlySet<HaInactiveKind> = new Set(["not_configured", "auth"]);

export function Inactive({ kind, onRetry, onOpenSettings }: { kind: HaInactiveKind; onRetry: () => void; onOpenSettings?: () => void }) {
  const copy = INACTIVE_COPY[kind];
  return (
    <div className="subsystem-inactive" role="region" aria-label="Home Assistant setup">
      <h2>{copy.title}</h2>
      <p>{copy.intro}</p>
      <ol>{copy.steps.map((s) => <li key={s}>{s}</li>)}</ol>
      <div className="subsystem-inactive__actions">
        {onOpenSettings && SETTINGS_KINDS.has(kind) && (
          <Button size="small" variant="primary" icon={Settings2} onClick={onOpenSettings}>Open Home Assistant settings</Button>
        )}
        <Button size="small" icon={RefreshCw} onClick={onRetry}>Check again</Button>
      </div>
    </div>
  );
}

/** Devices per group shown before the disclosure. */
const GROUP_GLANCE = 6;

export function SmartHomeField({ load = loadDevices, onOpenSettings }: { load?: () => Promise<DevicesState>; onOpenSettings?: () => void }) {
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

  const setClimate = async (device: HaEntity, service: "set_temperature" | "set_hvac_mode", data: HaServiceData) => {
    setRowError(null);
    if (!validServiceData("climate", service, data)) { setRowError({ entityId: device.entityId, message: HA_ERROR_COPY.invalid_input }); return; }
    setPending((p) => ({ ...p, [device.entityId]: true }));
    try {
      await haBackend().callService("climate", service, device.entityId, data);
      if (mounted.current) setState((s) => s.status === "ready" ? { ...s, devices: s.devices.map((d) => d.entityId !== device.entityId ? d
        : service === "set_hvac_mode" ? { ...d, state: data.hvacMode! } : { ...d, attributes: { ...d.attributes, temperature: data.temperature } }) } : s);
    } catch (error) {
      // Entity state is untouched on failure; ClimateControl re-syncs to it when pending clears.
      const kind = classifyHaError(error);
      if (mounted.current) setRowError({ entityId: device.entityId, message: HA_ERROR_COPY[kind] });
    } finally {
      if (mounted.current) setPending((p) => { const next = { ...p }; delete next[device.entityId]; return next; });
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

  const runScene = async (device: HaEntity) => {
    setRowError(null);
    setPending((p) => ({ ...p, [device.entityId]: true }));
    try { await haBackend().callService(device.domain, "turn_on", device.entityId); }
    catch (error) { const kind = classifyHaError(error); if (mounted.current) setRowError({ entityId: device.entityId, message: isServiceRefusal(kind) ? HA_ERROR_COPY[kind] : `Couldn't run ${displayName(device)}.` }); }
    finally { if (mounted.current) setPending((p) => { const next = { ...p }; delete next[device.entityId]; return next; }); }
  };

  const devices = state.status === "ready" ? state.devices : [];
  const groups = groupDevices(devices);
  const household = groups.filter((g) => g.id !== "system");
  const householdCount = household.reduce((n, g) => n + g.devices.length, 0);
  const system = groups.find((g) => g.id === "system")?.devices ?? [];
  const preview = sampleAdaptersEnabled();
  const deviceRow = (d: HaEntity) => (
                  <ListRow key={d.entityId} icon={ICONS[d.domain]} title={displayName(d)}
                    subtitle={rowError?.entityId === d.entityId ? <span role="alert" className="smart-home__row-error">{rowError.message}</span> : domainLabel(d.domain)}
                    value={isToggleable(d) || d.domain === "climate" || d.domain === "scene" || d.domain === "script" ? undefined : stateLabel(d)}
                    accessory={d.domain === "climate" ? <ClimateControl device={d} pending={!!pending[d.entityId]} onSend={(service, data) => { void setClimate(d, service, data); }} />
                      : d.domain === "scene" || d.domain === "script" ? <Button size="small" variant="secondary" disabled={!!pending[d.entityId]} onClick={() => { void runScene(d); }} aria-label={`Run ${displayName(d)}`}>Run</Button>
                      : isToggleable(d) ? <Toggle checked={d.state === "on"} disabled={!!pending[d.entityId]} aria-label={displayName(d)} onCheckedChange={(on) => { void toggle(d, on); }} /> : undefined} />
  );

  return (
    <div className="smart-home page-inset d18-page">
      <PageHero title="Smart home" statement={state.status === "ready" && devices.length ? `${householdCount} household device${householdCount === 1 ? "" : "s"}` : "Home Assistant"}
        detail={live ? <Badge className="smart-home__live" tone={LIVE_TONE[live]} role="status" aria-label={`Live updates: ${LIVE_COPY[live]}`}>{LIVE_COPY[live]}</Badge> : undefined} />
      <PageTake surface="smarthome" />
      {state.status === "loading" ? <p className="smart-home__meta" role="status">Loading devices…</p>
        : state.status === "inactive" ? <Inactive kind={state.kind} onRetry={refresh} onOpenSettings={onOpenSettings} />
          : live === "authFailed" ? <Inactive kind="auth" onRetry={refresh} />
          : state.status === "error" ? (
            <div className="smart-home__panel" role="region" aria-label="Devices">
              <p>Couldn't load devices.</p>
              <div><Button size="small" icon={RefreshCw} onClick={refresh}>Retry</Button></div>
            </div>
          ) : devices.length === 0 ? (
            <div className="smart-home__panel smart-home__empty" role="region" aria-label="Devices">
              <h2 className="smart-home__title">No devices yet.</h2>
              <p className="smart-home__meta">Add devices in Home Assistant and they'll show up here.</p>
            </div>
          ) : (
            <div className="smart-home__list">
              {preview && <SampleHomeControls />}
              {household.map((g) => (
                <Fragment key={g.id}>
                  <InsetGroup title={g.title}>{g.devices.slice(0, GROUP_GLANCE).map(deviceRow)}</InsetGroup>
                  {g.devices.length > GROUP_GLANCE && <More label={`${g.devices.length - GROUP_GLANCE} more ${g.title.toLowerCase()}`}><InsetGroup title={`More ${g.title.toLowerCase()}`}>{g.devices.slice(GROUP_GLANCE).map(deviceRow)}</InsetGroup></More>}
                </Fragment>
              ))}
              {household.length === 0 && <p className="smart-home__meta">No household devices yet. Only Home Assistant&apos;s own system sensors are connected.</p>}
            </div>
          )}
      <section className="smart-home__system" aria-labelledby="sh-system">
        <h2 id="sh-system" className="smart-home__title">System</h2>
        {system.length > 0 && <InsetGroup title="Backups and Home Assistant" footer="These are Home Assistant's own sensors, not household devices.">{system.map(deviceRow)}</InsetGroup>}
        <ContainerBar container={container} busy={busy} error={containerError} onStart={() => { void runContainer("start"); }} onStop={() => { void runContainer("stop"); }} />
        <p className="smart-home__meta">Changes come from Home Assistant as they happen.</p>
      </section>
    </div>
  );
}
