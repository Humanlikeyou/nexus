import { useMuseTelemetry } from "../neuro/useMuseTelemetry";
import { useNow } from "../shell/useNow";
import { ActionButton, EmptyState, Metric, PageHeading, SectionHeading, StatusIndicator } from "../design/ProductUI";
import { tone, number, date, time } from "../lib/format";
import { useTraining } from "../neuro/useTraining";
import { type MuseScreenProps, museSignalState } from "./muse";
import { Trace } from "./shared";
import { BrainCircuit, Bluetooth, ChevronDown, Play, Radio, RefreshCw, Square, Unplug } from "lucide-react";

export function NeuroField({ data, devices, operation, error, sessionStartedAt, sessionEndedAt, onScan, onConnect, onDisconnect, onStart, onStop, training }: MuseScreenProps & { training: ReturnType<typeof useTraining> }) {
  const telemetry = useMuseTelemetry(); // only this screen subscribes to live readings
  const now = useNow(); // 1 s tick scoped to this screen (was the app root clock)
  const sessions = data.sessions.filter((session) => session.device_type === "muse");
  const signal = museSignalState(data, telemetry);
  const elapsedSeconds = sessionStartedAt ? Math.max(0, Math.floor(((sessionEndedAt ?? now.getTime()) - sessionStartedAt) / 1000)) : 0;
  const elapsed = `${String(Math.floor(elapsedSeconds / 60)).padStart(2, "0")}:${String(elapsedSeconds % 60).padStart(2, "0")}`;
  const traceValues = telemetry.channels.TP9?.length ? telemetry.channels.TP9 : Object.values(telemetry.channels).find((values) => values.length) ?? [];
  const actions = data.muse.streaming || sessionStartedAt !== null
    ? <ActionButton icon={sessionEndedAt ? RefreshCw : Square} variant={sessionEndedAt ? "primary" : "danger"} onClick={onStop} disabled={operation !== null}>{sessionEndedAt ? "Retry save" : "Stop session"}</ActionButton>
    : data.muse.connected
      ? <><ActionButton icon={Play} variant="primary" onClick={onStart} disabled={operation !== null}>Start session</ActionButton><ActionButton icon={Unplug} onClick={onDisconnect} disabled={operation !== null}>Disconnect</ActionButton></>
      : <ActionButton icon={Bluetooth} variant="primary" onClick={onScan} disabled={operation !== null}>{operation === "scanning" ? "Searching..." : "Find Muse"}</ActionButton>;

  return <div className="system-field live-field">
    <PageHeading eyebrow="Neural interface" title="Live" description={data.muse.connected ? `${data.muse.device_name || "Muse headset"} is ${data.muse.streaming ? "recording" : "ready"}.` : "Connect a Muse headset for live EEG and neurofeedback sessions."} actions={actions} />
    <section className="training-strip" aria-label="Neurofeedback">
      <label htmlFor="training-protocol">Protocol<span className="protocol-select"><select id="training-protocol" value={training.protocolId} onChange={(event) => training.setProtocolId(event.target.value)} disabled={sessionStartedAt !== null || operation !== null}><option value="off">Recording only</option>{training.protocols.map((protocol) => <option key={protocol.id} value={protocol.id}>{protocol.name}</option>)}</select><ChevronDown size={16} aria-hidden="true" /></span></label>
      <div><span>Neurofeedback</span><StatusIndicator tone={training.state.phase === "held" ? "warning" : training.state.signal?.action === "reward" ? "success" : "neutral"}>{sessionStartedAt ? training.state.label : training.protocolId === "off" ? "Off" : "Ready to calibrate"}</StatusIndicator></div>
      {sessionStartedAt && training.state.phase === "calibrating" && <progress aria-label="Calibration" value={training.state.calibrationPercent} max={100} />}
      {training.state.phase === "training" && <><Metric label="Focus" value={number(training.state.focus)} /><Metric label="Relaxation" value={number(training.state.relaxation)} /></>}
    </section>
    {error && <div className="inline-alert inline-alert--danger" role="alert"><span>{error}</span><ActionButton variant="quiet" onClick={sessionStartedAt !== null ? onStop : onScan} disabled={operation !== null}>{sessionStartedAt !== null ? "Retry stop and save" : "Try again"}</ActionButton></div>}
    {sessionEndedAt !== null && <div className="inline-alert" role="status">Recording stopped at {elapsed}. Saving is pending; keep Nexus open and retry before starting another session.</div>}

    {!data.muse.connected ? <><div className="live-offline-layout">
        <EmptyState icon={Bluetooth} title={operation === "scanning" ? "Searching for Muse" : "No Muse connected"} detail={operation === "scanning" ? "Keep the headset nearby and in pairing mode. Scanning takes about five seconds." : "Put your headset in pairing mode, then let Nexus find it."} action={<ActionButton icon={RefreshCw} variant="primary" onClick={onScan} disabled={operation !== null}>{operation === "scanning" ? "Searching..." : "Search again"}</ActionButton>} />
        <section className="device-results" aria-live="polite">
          <SectionHeading title="Nearby headsets" detail={devices.length ? `${devices.length} Muse device${devices.length === 1 ? "" : "s"} found` : "Results appear here after a scan"} />
          {devices.map((device) => <article key={device.address}><div className="device-glyph"><BrainCircuit size={19} /></div><div><strong>{device.name}</strong><span>{device.rssi == null ? "Signal strength unavailable" : device.rssi > -65 ? "Strong signal" : device.rssi > -80 ? "Fair signal" : "Weak signal"}</span></div><ActionButton onClick={() => onConnect(device)} disabled={operation !== null}>{operation === "connecting" ? "Connecting..." : "Connect"}</ActionButton></article>)}
          {!devices.length && operation !== "scanning" && <p className="subtle-empty">No recent scan results.</p>}
        </section>
      </div><section className="live-readiness-strip" aria-label="Session readiness"><article><span>Muse</span><strong>Connection required</strong><p>Search for a nearby headset to begin.</p></article><article><span>WHOOP context</span><strong>{data.noop.status === "ready" ? "Current" : "Optional"}</strong><p>{data.noop.status === "ready" ? "Recovery context is ready for the session." : "A neural session can run without health context."}</p></article><article><span>Recording</span><strong>Local storage ready</strong><p>Completed sessions are saved on this Mac.</p></article></section></> : <div className="live-workspace">
      <section className="live-primary">
        <div className="live-status-line"><StatusIndicator tone={signal.tone}>{signal.label}</StatusIndicator><span>{data.muse.device_name || "Muse"}</span><span>{data.muse.streaming ? `${elapsed} elapsed` : "Session not started"}</span></div>
        <div className="live-readout">
          <div><span>Session</span><strong>{data.muse.streaming ? elapsed : "Ready"}</strong><p>{data.muse.streaming ? "Recording locally" : "Start when you are settled"}</p></div>
          <div><span>EEG samples</span><strong>{telemetry.sampleCount.toLocaleString()}</strong><p>{data.muse.streaming ? `${telemetry.packetCount.toLocaleString()} packets received` : "Waiting for a session"}</p></div>
          <div><span>WHOOP context</span><strong>{data.noop.today?.recovery == null ? "--" : `${number(data.noop.today.recovery)}%`}</strong><p>{data.noop.status === "ready" ? "Current recovery" : "No current recovery"}</p></div>
        </div>
        <section className="signal-chart">
          <SectionHeading title="EEG signal" detail={data.muse.streaming ? "TP9 · recent samples · visual update 4 Hz" : "Signal appears when the session starts"} />
          {traceValues.length > 1 ? <Trace values={traceValues.slice(-256)} tall /> : <div className="chart-empty"><Radio size={22} /><span>{data.muse.streaming ? "Acquiring the first signal packets." : "Start a session to view the live signal."}</span></div>}
        </section>
      </section>
      <aside className="live-inspector">
        <SectionHeading title="Frequency bands" detail="Relative power · 1–45 Hz" />
        <div className="band-list">{Object.entries(telemetry.bands).map(([band, value]) => <div key={band}><span>{band}</span><i><b style={{ width: `${Math.min(100, value)}%` }} /></i><strong>{data.muse.streaming && telemetry.sampleCount >= 256 ? `${value.toFixed(0)}%` : "--"}</strong></div>)}</div>
        <div className="inspector-note">Band power is a live signal summary, not a medical measurement.</div>
        <details className="diagnostics-disclosure"><summary>Signal diagnostics</summary><dl><div><dt>Last packet</dt><dd>{telemetry.lastPacketAt ? `${Math.max(0, Math.round((now.getTime() - telemetry.lastPacketAt) / 1000))}s ago` : "None"}</dd></div>{Object.entries(telemetry.channels).map(([channel, values]) => <div key={channel}><dt>{channel}</dt><dd>{values.length.toLocaleString()} buffered</dd></div>)}</dl></details>
      </aside>
    </div>}
    <section className="recent-session-rail"><SectionHeading title="Recent sessions" detail={`${sessions.length} recorded`} />{sessions.length ? <div>{sessions.slice(0, 4).map((session) => <article key={session.id}><time>{date(session.start)}</time><strong>{Math.max(1, Math.round(session.duration_secs / 60))} min</strong><span>{session.data_points.toLocaleString()} samples</span></article>)}</div> : <p className="subtle-empty">Your first completed Muse session will appear here.</p>}</section>
  </div>;
}
