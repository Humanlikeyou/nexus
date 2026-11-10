import { BrainCircuit, Bluetooth, Inbox, Play, RefreshCw, Square, Unplug, X } from "lucide-react";
import { useMemo, useState, useSyncExternalStore } from "react";
import { INITIAL_SNAPSHOT } from "../bridge";
import { dateTime } from "../lib/format";
import { BAND_KEYS, MIN_SAMPLES, createBandHistory, type BandKey } from "../neuro/bandHistory";
import { useMuseTelemetry } from "../neuro/useMuseTelemetry";
import type { useTraining } from "../neuro/useTraining";
import { useNow } from "../shell/useNow";
import type { RecordedSession } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, Meter, More, PageHero, Ring, Select, Sparkline, StatTile, type Tone } from "../ui";
import { recoveryBand } from "./HealthScreen";
import { parseBodyContext } from "../lib/noop";
import { hoursMinutes } from "../lib/mindBodyDeep";
import { score } from "../neuro/sessionSamples";
import { NeuroFlow } from "./NeuroFlow";
import { type MuseScreenProps, museSignalState } from "./muse";
import { LiveSignal } from "../neuro/LiveSignal";
import { recordingsAdapter } from "../neuro/recordings";
import "./NeuroScreen.css";

const MUSE_COMMAND = "get_muse_state";
const SESSIONS_COMMAND = "list_sessions";
const DASH = "—";

const BANDS: Record<BandKey, { symbol: string; name: string }> = {
  delta: { symbol: "δ", name: "Delta" },
  theta: { symbol: "θ", name: "Theta" },
  alpha: { symbol: "α", name: "Alpha" },
  beta: { symbol: "β", name: "Beta" },
  gamma: { symbol: "γ", name: "Gamma" },
};

const BUSY_LABEL = { scanning: "Searching…", connecting: "Connecting…", starting: "Starting…", stopping: "Stopping…", disconnecting: "Disconnecting…" } as const;

export function mmss(totalSeconds: number) {
  const s = Math.max(0, Math.floor(totalSeconds));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}
export function rssiText(rssi: number | null | undefined) {
  if (rssi == null) return "Signal strength unavailable";
  return rssi > -65 ? "Strong signal" : rssi > -80 ? "Fair signal" : "Weak signal";
}
const whole = (value: number | null | undefined) => (typeof value === "number" && Number.isFinite(value) ? Math.round(value) : null);

export type NeuroFieldProps = MuseScreenProps & {
  training: ReturnType<typeof useTraining>;
  /** Re-runs the snapshot load. Retry is hidden without it. */
  onRetry?: () => void;
  /** design-1110c C12: "Session history ›" on the Last session card. */
  onOpenHistory?: () => void;
  /** Overrides the first-load detection (refreshedAt still equals INITIAL_SNAPSHOT's). */
  loading?: boolean;
};

export function NeuroField({ data, devices, operation, error, sessionStartedAt, sessionEndedAt, onScan, onConnect, onDisconnect, onStart, onStop, training, onRetry, loading, onOpenHistory }: NeuroFieldProps) {
  const telemetry = useMuseTelemetry(); // only this screen subscribes to live readings
  const history = useBandHistory();
  const now = useNow(); // 1 s tick scoped to this screen
  const [selectedId, setSelectedId] = useState<string | null>(null);

  if (loading ?? data.refreshedAt === INITIAL_SNAPSHOT.refreshedAt) return <NeuroSkeleton />;

  const failed = data.unavailableSources.some((s) => s.command === MUSE_COMMAND);
  const sessionsFailed = data.unavailableSources.some((s) => s.command === SESSIONS_COMMAND);
  const { connected, streaming } = data.muse;
  const busy = operation !== null;
  const running = sessionStartedAt !== null;
  const pendingSave = sessionEndedAt !== null;
  const elapsedSeconds = running ? ((sessionEndedAt ?? now.getTime()) - sessionStartedAt) / 1000 : 0;
  const elapsed = mmss(elapsedSeconds);
  const signal = museSignalState(data, telemetry);
  const acquiring = streaming && telemetry.sampleCount < MIN_SAMPLES;
  const live = streaming && !acquiring;
  const sessions = data.sessions.filter((s) => s.device_type === "muse");
  const selected = sessions.find((s) => s.id === selectedId) ?? null;
  const state = training.state;
  const showCalm = running && training.protocolId !== "off";

  const deviceName = data.muse.device_name || "Muse";
  const deviceBadge: { tone: Tone; text: string } = !connected
    ? { tone: "neutral", text: `${deviceName} · Disconnected` }
    : streaming
      ? { tone: signal.tone === "danger" ? "danger" : "success", text: `${deviceName} · ${signal.tone === "danger" ? "Signal interrupted" : "Streaming"}` }
      : { tone: "info", text: `${deviceName} · Connected` };
  const recovery = data.noop.today?.recovery ?? null;
  const band = recoveryBand(recovery);

  // One prominent primary action; Disconnect only when connected and idle.
  const primary = pendingSave
    ? <Button variant="primary" icon={RefreshCw} onClick={onStop} loading={operation === "stopping"} disabled={busy}>{operation === "stopping" ? BUSY_LABEL.stopping : "Retry Save"}</Button>
    : running || streaming
      ? <Button variant="destructive" icon={Square} onClick={onStop} loading={operation === "stopping"} disabled={busy}>{operation === "stopping" ? BUSY_LABEL.stopping : "Stop session"}</Button>
      : connected
        ? <Button variant="primary" icon={Play} onClick={onStart} loading={operation === "starting"} disabled={busy}>{operation === "starting" ? BUSY_LABEL.starting : "Start session"}</Button>
        : <Button variant="primary" icon={Bluetooth} onClick={onScan} loading={operation === "scanning" || operation === "connecting"} disabled={busy || failed}>{operation === "scanning" || operation === "connecting" ? BUSY_LABEL[operation] : "Find Muse"}</Button>;
  const secondary = connected && !running && !streaming
    ? <Button variant="secondary" icon={Unplug} onClick={onDisconnect} loading={operation === "disconnecting"} disabled={busy}>{operation === "disconnecting" ? BUSY_LABEL.disconnecting : "Disconnect"}</Button>
    : null;

  const header = (
    <>
      <PageHero title="Neuro" statement={failed ? "Out of reach" : deviceBadge.text} detail={failed ? <Badge tone="danger">Unavailable</Badge> : band ? <Badge tone={band.tone}>Recovery {Math.round(recovery as number)}%</Badge> : undefined} />
      {secondary && <div className="neuro__actions">{secondary}</div>}
    </>
  );

  if (failed) {
    return (
      <div className="neuro" data-state="error">
        {header}
        <Card className="neuro__alert" data-tone="danger" role="alert" title="Muse status couldn't be read" headingLevel={2}>
          <p className="neuro__callout">Amara OS couldn't reach the Muse bridge. Try again in a moment.</p>
          {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Try Again</Button>}
        </Card>
      </div>
    );
  }

  const protocolOptions = [{ value: "off", label: "Recording only" }, ...training.protocols.map((p) => ({ value: p.id, label: p.name }))];
  const statusBadge: { tone: Tone; text: string } = !running
    ? { tone: "neutral", text: training.protocolId === "off" ? "Recording only" : "Ready to calibrate" }
    : state.phase === "held"
      ? { tone: "warning", text: state.label }
      : { tone: state.signal?.action === "reward" ? "success" : "neutral", text: state.label };

  const liveAux = recordingsAdapter(data.sessions).liveAux();
  const sinceLastPacket = telemetry.lastPacketAt ? Math.max(0, Math.round((now.getTime() - telemetry.lastPacketAt) / 1000)) : null;
  const heroIdle = connected && !streaming && !running;

  return (
    <div className="neuro" data-state={!connected ? "empty" : streaming ? "streaming" : "ready"}>
      {header}

      {error && (
        <Card className="neuro__alert" data-tone="danger" role="alert">
          <p className="neuro__callout">{error}</p>
          <Button variant="secondary" size="small" onClick={running ? onStop : onScan} disabled={busy}>{running ? "Retry Stop and Save" : "Try Again"}</Button>
        </Card>
      )}
      {pendingSave && (
        <Card className="neuro__alert" data-tone="warning" role="status">
          <p className="neuro__callout">Recording stopped at <span className="neuro__num">{elapsed}</span>. Saving is pending; keep Amara OS open.</p>
        </Card>
      )}

      {!connected
        ? (
          <>
          <Card className="neuro__empty" title={operation === "scanning" ? "Searching for Muse" : "No Muse connected"} headingLevel={2}>
            <div className="neuro__empty-lead">
              <BrainCircuit className="neuro__empty-icon" aria-hidden="true" />
              <p className="neuro__callout">{operation === "scanning" ? "Keep the headset nearby. Scanning takes about five seconds." : "Put your headset in pairing mode"}</p>
              <div className="neuro__empty-action">{primary}</div>
            </div>
            <InsetGroup title="Nearby headsets" className="neuro__group" aria-live="polite" footer={devices.length ? undefined : operation === "scanning" ? "Looking for headsets…" : "Results appear here after a scan."}>
              {devices.map((device) => (
                <ListRow key={device.address} icon={BrainCircuit} title={device.name} subtitle={rssiText(device.rssi)}
                  accessory={<Button size="small" onClick={() => onConnect(device)} disabled={busy} loading={operation === "connecting"}>{operation === "connecting" ? BUSY_LABEL.connecting : "Connect"}</Button>} />
              ))}
            </InsetGroup>
          </Card>
          {data.sessions[0] && <LastSessionCard session={data.sessions[0]} onOpenHistory={onOpenHistory} />}
          </>
        )
        : (
          <>
            <NeuroFlow training={training} telemetry={telemetry} sessions={data.sessions} running={running} action={primary} />
            <More label="Live readings and signal" open={running && !training.state.stage ? true : undefined}>
            <Card className="neuro__strip" aria-label="Session">
              <Select className="neuro__protocol" label="Protocol" hideLabel value={training.protocolId} options={protocolOptions}
                onChange={(e) => training.setProtocolId(e.target.value)} disabled={running || busy} />
              <Badge tone={statusBadge.tone}>{statusBadge.text}</Badge>
              {running && state.phase === "calibrating" && <Meter className="neuro__calibration" label="Calibration" value={state.calibrationPercent} valueText={`${Math.round(state.calibrationPercent)}% calibrated`} />}
              {streaming && signal.tone === "danger" && <Badge tone="danger">Signal interrupted</Badge>}
            </Card>

            <div className="neuro__hero">
              <StatTile className="neuro__session-time" label="Session" value={heroIdle ? "Ready" : elapsed} />
              {showCalm && (
                <Card className="neuro__calm" title={<>Calm <Badge tone="neutral">Beta</Badge></>} headingLevel={3} elevation={0}>
                  <Ring value={whole(state.calmPercent)} label="Calm" size="sm" color="var(--accent)" valueText={whole(state.calmPercent) == null ? undefined : `${whole(state.calmPercent)}% calm`}>
                    <span className="neuro__ring-value">{whole(state.calmPercent) == null ? DASH : `${whole(state.calmPercent)}%`}</span>
                  </Ring>
                </Card>
              )}
              <StatTile label="Focus" value={heroIdle ? "Ready" : whole(state.focus)} />
              <StatTile label="Calm" value={heroIdle ? "Ready" : whole(state.relaxation)} />
            </div>

            <div className="neuro__row">
              <Card title="Band powers" className="neuro__bands">
                <InsetGroup className="neuro__group">
                  {BAND_KEYS.map((key) => {
                    const value = live ? telemetry.bands[key] : 0;
                    return (
                      <ListRow key={key} className="neuro__band" title={<><span className="neuro__band-symbol" aria-hidden="true">{BANDS[key].symbol}</span>{BANDS[key].name}</>}
                        subtitle={<Meter size="sm" label={`${BANDS[key].name} relative power`} value={value} valueText={live ? `${Math.round(value)}%` : "No reading"} />}
                        value={<span className="neuro__num">{acquiring ? "Acquiring…" : live ? `${Math.round(value)}%` : DASH}</span>}
                        accessory={<Sparkline className="neuro__band-trend" values={history[key]} label={`${BANDS[key].name}, last 60 s`} format={(v) => `${Math.round(v)}%`} />} />
                    );
                  })}
                </InsetGroup>
                <p className="neuro__footnote">Relative power · 1–45 Hz · not a medical measurement.</p>
              </Card>

              <LiveSignal trace={telemetry.trace ?? telemetry.channels} aux={liveAux} streaming={streaming}>
                <details className="neuro__disclosure">
                  <summary>Signal diagnostics</summary>
                  <InsetGroup className="neuro__group">
                    <ListRow title="Last packet" value={<span className="neuro__num">{sinceLastPacket == null ? "None" : `${sinceLastPacket}s ago`}</span>} />
                    {Object.entries(telemetry.channels).map(([channel, values]) => (
                      <ListRow key={channel} title={channel} value={<span className="neuro__num">{values.length.toLocaleString()} buffered</span>} />
                    ))}
                  </InsetGroup>
                </details>
              </LiveSignal>
            </div>
            </More>
          </>
        )}

      <div className="neuro__row neuro__row--sessions">
        <More label="Recent sessions">
        <InsetGroup title="Recent sessions" className="neuro__sessions"
          footer={sessionsFailed ? "Saved sessions couldn't be read." : sessions.length ? undefined : "Your first completed session will appear here."}>
          {sessions.slice(0, 5).map((s) => (
            <ListRow key={s.id} icon={Inbox} title={dateTime(s.start)} onSelect={() => setSelectedId(s.id === selectedId ? null : s.id)} selected={s.id === selectedId}
              subtitle={<span className="neuro__num">{Math.max(1, Math.round(s.duration_secs / 60))} min · {s.data_points.toLocaleString()} samples · calm {whole(s.summary.avg_relaxation) ?? DASH}</span>} />
          ))}
        </InsetGroup>
        </More>
        {selected && <SessionPanel session={selected} onClose={() => setSelectedId(null)} />}
      </div>
    </div>
  );
}

function SessionPanel({ session, onClose }: { session: RecordedSession; onClose: () => void }) {
  const s = session.summary;
  return (
    <Card as="aside" className="neuro__panel" title="Session summary" headingLevel={2}
      accessory={<Button variant="plain" size="small" iconOnly icon={X} aria-label="Close session summary" onClick={onClose} />}>
      <p className="neuro__callout">{dateTime(session.start)} · <span className="neuro__num">{mmss(session.duration_secs)}</span></p>
      <div className="neuro__panel-tiles">
        <StatTile label={<>Calm <Badge tone="neutral">Beta</Badge></>} value={whole(s.calm_pct)} unit="%" />
        <StatTile label="In target" value={whole(s.time_in_target_pct)} unit="%" />
        <StatTile label="Avg focus" value={whole(s.avg_focus)} />
        <StatTile label="Avg calm" value={whole(s.avg_relaxation)} />
      </div>
      <p className="neuro__footnote"><span className="neuro__num">{session.data_points.toLocaleString()}</span> samples recorded on this Mac.</p>
    </Card>
  );
}

const whenFmt = new Intl.DateTimeFormat("en-US", { weekday: "short", month: "short", day: "numeric" });
const hhmm = (iso: string) => new Date(iso).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });

/** C12: a thin 0–100 meter with its scale; the number is in the header, so this is never colour alone. */
function ScoreMeter({ label, value }: { label: string; value: number | null }) {
  return (
    <div className="neuro__score">
      <p className="neuro__score-head"><span>{label}</span><strong>{value == null ? DASH : Math.round(value)}<small> / 100</small></strong></p>
      <div className="neuro__score-bar" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={value == null ? undefined : Math.round(value)}>
        <i style={{ width: `${Math.max(0, Math.min(100, value ?? 0))}%` }} />
      </div>
      <p className="neuro__score-scale" aria-hidden="true"><span>0</span><span>50</span><span>100</span></p>
    </div>
  );
}

/**
 * design-1110c C12: the latest saved session when no Muse is connected: focus and calm in the
 * header pattern with thin meters, the body context of that day, and a pointer to its saved recording
 * in Neuro history (design-1110e E10).
 */
function LastSessionCard({ session, onOpenHistory }: { session: RecordedSession; onOpenHistory?: () => void }) {
  const body = parseBodyContext(session.body_context);
  const minutes = Math.max(1, Math.round(session.duration_secs / 60));
  const figures: Array<[string, string | null, string]> = [
    ["Recovery that day", body?.recovery == null ? null : String(Math.round(body.recovery)), "%"],
    ["HRV", body?.hrv_ms == null ? null : String(Math.round(body.hrv_ms)), "ms"],
    ["Resting HR", body?.resting_hr == null ? null : String(Math.round(body.resting_hr)), "bpm"],
    ["Sleep before", body?.sleep_minutes == null ? null : hoursMinutes(body.sleep_minutes), ""],
  ];
  return (
    <Card className="neuro__last" title="Last session" headingLevel={2}
      accessory={<span className="neuro__last-when">{`${whenFmt.format(new Date(session.start))} · ${hhmm(session.start)}–${hhmm(session.end)} · ${minutes} min`}</span>}>
      <div className="neuro__last-scores">
        <ScoreMeter label="Average focus" value={score(session.summary.avg_focus)} />
        <ScoreMeter label="Average calm" value={score(session.summary.avg_relaxation)} />
      </div>
      {body && (
        <dl className="neuro__last-body">
          {figures.map(([label, value, unit]) => <div key={label}><dd>{value ?? DASH}{value && unit ? <small> {unit}</small> : null}</dd><dt>{label}</dt></div>)}
        </dl>
      )}
      <p className="neuro__last-foot">
        <span>{session.file_path ? "The raw signal of this session is saved on this Mac. Open it in Neuro history." : "This session has its summary only."}</span>
        {onOpenHistory && <Button variant="plain" size="small" onClick={onOpenHistory}>Session history ›</Button>}
      </p>
    </Card>
  );
}

/** 60 s band history (Decision 7); one buffer per mounted Neuro screen. */
function useBandHistory() {
  const store = useMemo(() => createBandHistory(), []);
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}

function NeuroSkeleton() {
  return (
    <div className="neuro" data-state="loading" aria-busy="true" aria-label="Loading Neuro">
      <header className="neuro__header"><div className="neuro__title-row"><h1>Neuro</h1><span className="neuro__skel neuro__skel--badge" /></div></header>
      <span className="neuro__skel neuro__skel--strip" />
      <div className="neuro__hero">{[0, 1, 2].map((i) => <span key={i} className="neuro__skel neuro__skel--tile" />)}</div>
      <div className="neuro__row">
        <Card title="Band powers"><span className="neuro__skel neuro__skel--block" /></Card>
        <Card title="Signal"><span className="neuro__skel neuro__skel--block" /></Card>
      </div>
    </div>
  );
}
