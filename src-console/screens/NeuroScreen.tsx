import { CalibrationView } from "../neuro/CalibrationView";
import { TrainingView } from "../neuro/TrainingView";
import { BrainCircuit, Bluetooth, Inbox, Play, RefreshCw, Square, Unplug, X } from "lucide-react";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { INITIAL_SNAPSHOT } from "../bridge";
import { dateTime } from "../lib/format";
import { parseBodyContext } from "../lib/noop";
import { BAND_KEYS, MIN_SAMPLES, createBandHistory, type BandKey } from "../neuro/bandHistory";
import { useMuseTelemetry } from "../neuro/useMuseTelemetry";
import type { useTraining } from "../neuro/useTraining";
import { useNow } from "../shell/useNow";
import type { RecordedSession } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, Meter, More, PageHero, Ring, Select, Sparkline, StatTile, type Tone } from "../ui";
import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import { recoveryBand } from "./HealthScreen";
import { NeuroFlow } from "./NeuroFlow";
import { type MuseScreenProps, museSignalState } from "./muse";
import { LiveSignal } from "../neuro/LiveSignal";
import { useRecordingsAdapter } from "../neuro/useRecordingsAdapter";
import { HemisyncSection } from "./HemisyncSection";
import { NeuroTrends } from "./NeuroTrends";
import "./NeuroScreen.css";
import { MuseJourney } from "../design1118/MuseJourney";
import { useSessionBands } from "../neuro/useSessionBands";

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
  /** The route's view: "neuro/calibration" opens the optional pre-meditation calibration (decision 0058). */
  view?: string;
  onOpenCalibration?: () => void;
  onCloseCalibration?: () => void;
  /** Decision 0059: "neuro/training" opens protocol settings and session records. */
  onOpenTraining?: () => void;
  training: ReturnType<typeof useTraining>;
  /** Re-runs the snapshot load. Retry is hidden without it. */
  onRetry?: () => void;
  /** design-1110c C12: "Session history ›" on the Last session card. */
  onOpenHistory?: () => void;
  /** Muse session id of the running recording (Hemi-Sync links its track to it). */
  sessionId?: string | null;
  /** The last search found no headset: the note and "Search again" stay until one is found. */
  scanEmpty?: boolean;
  /** The session Stop just saved: a "Session saved" card shows until Done. */
  lastSavedId?: string | null;
  onDismissSaved?: () => void;
  /** Opens one saved session's recording in Neuro history (App's navigate). */
  onOpenRecording?: (sessionId: string) => void;
  /** Bumped by Neuro history's "Start session": focuses Find Muse or Start session once the page is up. */
  focusPrimary?: number;
  /** Overrides the first-load detection (refreshedAt still equals INITIAL_SNAPSHOT's). */
  loading?: boolean;
};

export function NeuroField(props: NeuroFieldProps) {
  // Decision 0058: Neuro › Calibration is its own view ("neuro/calibration"), reached from the header.
  if (props.view === "neuro/calibration") return <div className="neuro page-inset"><CalibrationView onDone={props.onCloseCalibration} /></div>;
  if (props.view === "neuro/training") return <div className="neuro page-inset"><TrainingView training={props.training} running={props.sessionStartedAt != null} onDone={props.onCloseCalibration} /></div>;
  return <NeuroMain {...props} />;
}

function NeuroMain({ data, devices, operation, error, sessionId = null, sessionStartedAt, sessionEndedAt, onScan, onConnect, onDisconnect, onStart, onStop, training, onRetry, loading, onOpenHistory, scanEmpty = false, lastSavedId = null, onDismissSaved, onOpenRecording, focusPrimary = 0, onOpenCalibration, onOpenTraining }: NeuroFieldProps) {
  const telemetry = useMuseTelemetry(); // only this screen subscribes to live readings
  const history = useBandHistory();
  const now = useNow(); // 1 s tick scoped to this screen
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const recordings = useRecordingsAdapter(data.sessions);
  const withBands = useSessionBands(data.sessions); // band series for the newest few sessions (the brain rings)
  useFocusPrimary(focusPrimary);

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
  const sessions = withBands.filter((s) => s.device_type === "muse");
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
      <PageHero title="Neuro" statement={failed ? "Out of reach" : deviceBadge.text} detail={failed ? <Badge tone="danger">Unavailable</Badge> : band ? <Badge tone={band.tone}>Recovery now {Math.round(recovery as number)}%</Badge> : undefined} />
      {(secondary || (onOpenCalibration && !running && !streaming)) && <div className="neuro__actions">{secondary}{onOpenCalibration && !running && !streaming && <Button variant="plain" onClick={onOpenCalibration}>Calibrate (optional)</Button>}{onOpenTraining && !running && !streaming && <Button variant="plain" onClick={onOpenTraining}>Training</Button>}</div>}
    </>
  );

  if (failed) {
    return (
      <div className="neuro page-inset" data-state="error">
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

  const liveAux = recordings.liveAux();
  const sinceLastPacket = telemetry.lastPacketAt ? Math.max(0, Math.round((now.getTime() - telemetry.lastPacketAt) / 1000)) : null;
  const heroIdle = connected && !streaming && !running;
  const saved = lastSavedId && !running ? data.sessions.find((s) => s.id === lastSavedId) ?? null : null;
  const savedBody = saved ? parseBodyContext(saved.body_context) : null;
  const lastSession = sessions[0] ?? withBands[0] ?? null;
  const recoveryNow = typeof data.noop.today?.recovery === "number" ? data.noop.today.recovery : null;
  // Live band power for the "Now" ring: only while Muse is really streaming (no second BLE connection; same telemetry as the readings below).
  // Decision 0057: bands are a reading only when all four channels have a second of samples; otherwise a dash, never 0 %.
  const bandsLive = live && telemetry.bandsValid !== false;
  const liveBands = bandsLive ? telemetry.bands : null;

  return (
    <div className="neuro page-inset" data-state={!connected ? "empty" : streaming ? "streaming" : "ready"}>
      {header}
      {!running && !streaming && <AmaraTake page="neuro" className="neuro__take" />}

      {error && (
        <Card className="neuro__alert" data-tone="danger" role="alert">
          <p className="neuro__callout">{error}</p>
          <Button variant="secondary" size="small" onClick={running ? onStop : onScan} disabled={busy}>{running ? "Retry Stop and Save" : "Try Again"}</Button>
        </Card>
      )}
      {lastSavedId && !running && (
        <Card className="neuro__alert" role="status" title="Session saved" headingLevel={2}>
          <p className="neuro__callout">
            {saved ? <>{Math.max(1, Math.round(saved.duration_secs / 60))} min · focus {whole(saved.summary.avg_focus) ?? DASH} · calm {whole(saved.summary.avg_relaxation) ?? DASH}</> : "Saved on this Mac. Reading it back…"}
            {savedBody ? <> · recovery that day {savedBody.recovery != null ? `${Math.round(savedBody.recovery)}%` : DASH}{savedBody.hrv_ms != null ? ` · HRV ${Math.round(savedBody.hrv_ms)} ms` : ""}</> : null}
          </p>
          <p className="neuro__footnote">Amara&apos;s Neuro take is updating with this session. It appears below the headline when it is ready.</p>
          <div className="neuro__actions">
            {onOpenRecording && <Button variant="primary" size="small" onClick={() => onOpenRecording(lastSavedId)}>Open recording ›</Button>}
            <Button variant="secondary" size="small" onClick={onDismissSaved}>Done</Button>
          </div>
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
          {/* design-1118a: last session is the hero; recovery that day and recovery now stay separate. */}
          <MuseJourney session={lastSession} sessions={sessions.length ? sessions : data.sessions}
            recoveryNow={recoveryNow} onOpenHistory={onOpenHistory} onOpenSession={onOpenRecording}
            status={<span className="d18-chip" role="status">{operation === "scanning" ? "Searching for Muse" : "No Muse connected"}</span>} action={primary}
            devices={(devices.length > 0 || operation === "scanning") ? (
              <InsetGroup title="Nearby headsets" className="neuro__group" aria-live="polite" footer={devices.length ? undefined : "Looking for headsets… Scanning takes about five seconds."}>
                {devices.map((device) => (
                  <ListRow key={device.address} icon={BrainCircuit} title={device.name} subtitle={rssiText(device.rssi)}
                    accessory={<Button size="small" onClick={() => onConnect(device)} disabled={busy} loading={operation === "connecting"}>{operation === "connecting" ? BUSY_LABEL.connecting : "Connect"}</Button>} />
                ))}
              </InsetGroup>
            ) : scanEmpty ? (
              <div className="d18-note" role="status" aria-live="polite">
                <p>No Muse found. Put it in pairing mode (hold the power button until the light pulses), then search again.</p>
                <Button size="small" variant="secondary" icon={Bluetooth} onClick={onScan} disabled={busy || failed}>Search again</Button>
              </div>
            ) : null} />
          </>
        )
        : (
          <>
            {(heroIdle || (live && !running)) && !pendingSave && lastSession && (
              <MuseJourney hideNext liveBands={liveBands} session={lastSession} sessions={sessions.length ? sessions : data.sessions} recoveryNow={recoveryNow} onOpenHistory={onOpenHistory} onOpenSession={onOpenRecording} action={null} />
            )}
            <NeuroFlow training={training} telemetry={telemetry} sessions={data.sessions} running={running} action={primary} savedSessionId={lastSavedId} />
            <More label="Live readings and signal" open={(running && !training.state.stage) || streaming ? true : undefined}>
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
                    const value = bandsLive ? telemetry.bands[key] : 0;
                    return (
                      <ListRow key={key} className="neuro__band" title={<><span className="neuro__band-symbol" aria-hidden="true">{BANDS[key].symbol}</span>{BANDS[key].name}</>}
                        subtitle={<Meter size="sm" label={`${BANDS[key].name} relative power`} value={value} valueText={bandsLive ? `${Math.round(value)}%` : "No reading"} />}
                        value={<span className="neuro__num">{acquiring ? "Acquiring…" : bandsLive ? `${Math.round(value)}%` : DASH}</span>}
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

      <HemisyncSection connected={connected} recording={running && !pendingSave} sessionId={sessionId} operation={operation} protocolId={training.protocolId} setProtocolId={training.setProtocolId} onStartMuse={onStart} onStopMuse={onStop} onFindMuse={onScan} onOpenRecording={onOpenRecording} />
      <NeuroTrends recordings={recordings} />

      <div className="neuro__row neuro__row--sessions">
        <InsetGroup title="Recent sessions" className="neuro__sessions"
          footer={sessionsFailed ? "Saved sessions couldn't be read." : sessions.length ? undefined : "Your first completed session will appear here."}>
          {sessions.slice(0, 5).map((s) => (
            <ListRow key={s.id} icon={Inbox} title={dateTime(s.start)} onSelect={() => setSelectedId(s.id === selectedId ? null : s.id)} selected={s.id === selectedId}
              subtitle={<span className="neuro__num">{Math.max(1, Math.round(s.duration_secs / 60))} min · {s.data_points.toLocaleString()} samples · calm {whole(s.summary.avg_relaxation) ?? DASH}</span>} />
          ))}
        </InsetGroup>
        {selected && <SessionPanel session={selected} onClose={() => setSelectedId(null)} onOpen={onOpenRecording} />}
      </div>
    </div>
  );
}

function SessionPanel({ session, onClose, onOpen }: { session: RecordedSession; onClose: () => void; onOpen?: (sessionId: string) => void }) {
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
      {onOpen && <Button variant="secondary" size="small" onClick={() => onOpen(session.id)}>Open recording ›</Button>}
    </Card>
  );
}


/** Focuses the page's one primary action (Find Muse / Start session) when `signal` is raised; waits for the page to render. */
function useFocusPrimary(signal: number) {
  useEffect(() => {
    if (!signal) return;
    let frame = 0, tries = 0;
    const go = () => {
      const target = [...document.querySelectorAll<HTMLButtonElement>(".neuro button")].find((b) => /^(Find Muse|Start session)$/.test(b.textContent?.trim() ?? "") && !b.disabled);
      if (target) target.focus();
      else if (tries++ < 90) frame = requestAnimationFrame(go);
    };
    frame = requestAnimationFrame(go);
    return () => cancelAnimationFrame(frame);
  }, [signal]);
}

/** 60 s band history (Decision 7); one buffer per mounted Neuro screen. */
function useBandHistory() {
  const store = useMemo(() => createBandHistory(), []);
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}

function NeuroSkeleton() {
  return (
    <div className="neuro page-inset" data-state="loading" aria-busy="true" aria-label="Loading Neuro">
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
