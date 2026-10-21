import { BiometricReadout } from "../design/SystemPrimitives";
import { tone, number, date, time, formatMinutes } from "../lib/format";
import type { NoopHealthInsight, SystemSnapshot } from "../types";
import { State, Trace } from "./shared";
import { HeartPulse } from "lucide-react";

export function HealthField({ data }: { data: SystemSnapshot }) {
  const health = data.noop;
  const today = health.today;
  const history = health.recentDays;
  const recoveryTrend = history.flatMap((item) => item.recovery == null ? [] : [item.recovery]);
  const hrvTrend = history.flatMap((item) => item.avgHrv == null ? [] : [item.avgHrv]);
  const recoveryLabels = history.flatMap((item) => item.recovery == null ? [] : [item.day.slice(5)]);
  const hrvLabels = history.flatMap((item) => item.avgHrv == null ? [] : [item.day.slice(5)]);
  const overnight = health.latestNight ?? health.latestSleep;
  const overnightSleepMinutes = overnight?.durationMin ?? today?.totalSleepMin ?? null;
  const overnightHrv = overnight?.avgHrv ?? today?.avgHrv ?? null;
  const overnightRhr = overnight?.restingHr ?? today?.restingHr ?? null;
  const overnightEfficiency = overnight?.efficiency ?? today?.efficiency ?? null;
  const sleepHours = overnightSleepMinutes == null ? null : overnightSleepMinutes / 60;
  const healthReady = health.status === "ready";
  const awaitingNightlyData = health.status === "waiting_for_nightly_data";
  const canShowHistoricalSignals = health.status !== "unavailable";
  const sleepDurationLabel = overnightSleepMinutes == null ? "--" : `${Math.floor(overnightSleepMinutes / 60)}h ${Math.round(overnightSleepMinutes % 60)}m`;
  const sleepStages = [
    { label: "Light", minutes: overnight?.lightMin ?? 0, tone: "light" },
    { label: "Deep", minutes: overnight?.deepMin ?? 0, tone: "deep" },
    { label: "REM", minutes: overnight?.remMin ?? 0, tone: "rem" },
  ].filter((stage) => stage.minutes > 0);
  const fallbackInsights: NoopHealthInsight[] = [];

  if (overnightSleepMinutes != null) {
    fallbackInsights.push({
      metric: "latest_sleep",
      fact: `${sleepDurationLabel} of sleep`,
      context: overnight
        ? `NOOP recorded the latest night from ${time(overnight.startedAt)} to ${time(overnight.endedAt)}.`
        : "Taken from the latest completed NOOP daily record.",
    });
  }
  if (sleepStages.length > 0 && overnight) {
    const stageFacts = sleepStages.map((stage) => `${formatMinutes(stage.minutes)} ${stage.label.toLowerCase()}`);
    const context = [
      overnight.sessionCount > 1 ? `Combined across ${overnight.sessionCount} sleep sessions.` : "From last night's sleep session.",
      overnight.awakeMin && overnight.awakeMin > 0 ? `${formatMinutes(overnight.awakeMin)} was staged awake within the sleep window.` : null,
    ].filter(Boolean).join(" ");
    fallbackInsights.push({
      metric: "sleep_stages",
      fact: stageFacts.join(", "),
      context,
    });
  }
  if (overnightHrv != null || overnightRhr != null) {
    const readings = [
      overnightHrv == null ? null : `HRV ${number(overnightHrv)} ms`,
      overnightRhr == null ? null : `resting heart rate ${number(overnightRhr)} bpm`,
    ].filter(Boolean).join(", ");
    fallbackInsights.push({
      metric: "overnight_signals",
      fact: readings.charAt(0).toUpperCase() + readings.slice(1),
      context: "These are measured overnight signals from the latest NOOP sleep record.",
    });
  }
  if (health.battery) {
    fallbackInsights.push({
      metric: "whoop_battery",
      fact: `WHOOP battery is ${number(health.battery.percentage)}%`,
      context: `${health.battery.charging === true ? "Charging" : health.battery.charging === false ? "On strap" : "Charge state unavailable"}; last observed ${date(health.battery.observedAt)}.`,
    });
  }

  const visibleInsights = [...health.insights];
  for (const insight of fallbackInsights) {
    if (!visibleInsights.some((existing) => existing.metric === insight.metric)) visibleInsights.push(insight);
  }

  const secondarySignals = [
    { label: "RHR", value: number(overnightRhr), unit: "BPM" },
    today?.strain != null
      ? { label: "Strain", value: number(today.strain, 1), unit: "" }
      : { label: "Efficiency", value: number(overnightEfficiency == null ? null : overnightEfficiency * 100), unit: "%" },
    today?.respRateBpm != null
      ? { label: "Respiratory", value: number(today.respRateBpm, 1), unit: "RPM" }
      : { label: "Battery", value: number(health.battery?.percentage), unit: "%" },
    today?.spo2Pct != null
      ? { label: "SpO2", value: number(today.spo2Pct, 1), unit: "%" }
      : { label: "Data age", value: health.dataAgeDays == null ? "--" : String(health.dataAgeDays), unit: "D" },
  ];

  if (!canShowHistoricalSignals) {
    return (
      <div className="system-field health-field health-field--awaiting">
        <section className="health-presence">
          <div><span className="system-kicker">NOOP health</span><h1>Health</h1><p>Read directly from your local NOOP store.</p></div>
          <div className="health-presence-status"><State value={health.status} /><span>Awaiting a NOOP record</span></div>
        </section>
        <section className="health-awaiting">
          <div className="health-awaiting-lead">
            <span className="system-kicker">LOCAL DATA STATUS</span>
            <h2>Awaiting the first daily record.</h2>
            <p>{health.message || "NOOP health data is not available yet."}</p>
          </div>
          <div className="health-awaiting-signals" aria-label="Health signals awaiting NOOP data">
            <div><span>Recovery</span><strong>--</strong><small>Nightly readiness</small></div>
            <div><span>HRV</span><strong>--<small>ms</small></strong><small>Overnight variability</small></div>
            <div><span>Sleep</span><strong>--<small>h</small></strong><small>Restorative sleep</small></div>
          </div>
          <div className="health-awaiting-readiness" aria-label="NOOP integration readiness">
            <article><span>Data access</span><strong>Local store missing</strong><p>Terrence has no NOOP daily record to read yet.</p></article>
            <article><span>Morning briefing</span><strong>Waiting for nightly data</strong><p>Amara stays quiet until a fresh sleep session is available.</p></article>
            <article><span>Battery watch</span><strong>Waiting for device state</strong><p>No WHOOP battery reading is available locally.</p></article>
          </div>
        </section>
      </div>
    );
  }

  if (awaitingNightlyData) {
    return (
      <div className="system-field health-field health-field--awaiting-nightly">
        <section className="health-presence">
          <div><span className="system-kicker">NOOP health</span><h1>Health</h1><p>Read directly from your local NOOP store.</p></div>
          <div className="health-presence-status"><State value={health.status} /><span>Awaiting overnight sync</span></div>
        </section>
        <section className="health-sync-field">
          <div className="health-sync-lead">
            <span className="system-kicker">LIVE SIGNAL STATUS</span>
            <h2>NOOP is connected. The overnight picture is still forming.</h2>
            <p>{health.message}</p>
            <div className="health-sync-stamp"><span>Daily record</span><strong>{health.latestDay || "Awaiting date"}</strong><small>Strain {number(today?.strain, 1)} is the only complete signal so far.</small></div>
          </div>
          <div className="health-sync-readouts" aria-label="Nightly health signals awaiting synchronization">
            <div><span>Recovery</span><strong>--</strong><small>Score arrives after the sleep record closes.</small></div>
            <div><span>HRV</span><strong>--<small>ms</small></strong><small>Overnight variability is not available yet.</small></div>
            <div><span>Sleep</span><strong>--<small>h</small></strong><small>Waiting for a completed sleep session.</small></div>
          </div>
        </section>
        <section className="health-sync-rail" aria-label="NOOP health automation readiness">
          <article><span>Local source</span><strong>NOOP record is current</strong><p>Terrence is reading the local store directly. No second WHOOP source is active.</p></article>
          <article><span>Morning briefing</span><strong>Held until sleep sync</strong><p>Amara will only speak after a fresh overnight record has meaningful data.</p></article>
          <article><span>Battery watch</span><strong>{health.battery ? `${number(health.battery.percentage)}% on strap` : "Waiting for current telemetry"}</strong><p>{health.battery ? "A current battery reading is available through NOOP." : "Amara stays quiet until NOOP provides a fresh battery reading."}</p></article>
        </section>
      </div>
    );
  }

  return (
    <div className="system-field health-field health-field--ready">
      <section className="health-presence">
        <div><span className="system-kicker">NOOP health</span><h1>Health</h1><p>Read directly from your local NOOP store.</p></div>
        <div className="health-presence-status"><State value={health.status} /><span>{awaitingNightlyData ? "Awaiting overnight sync" : overnight ? `Latest night ${overnight.day}` : health.latestDay ? `Latest daily record ${health.latestDay}` : "Awaiting a NOOP record"}</span></div>
      </section>

      <section className="health-arcs">
        <BiometricReadout value={today?.recovery} label="Recovery" tint="lime" />
        <BiometricReadout value={overnightHrv} max={120} label="HRV" unit="ms" />
        <BiometricReadout value={sleepHours} max={10} label="Sleep" unit="h" digits={1} tint="amber" />
      </section>

      <section className="health-trace">
        <header><span>Recovery history</span><State value={healthReady ? "current" : health.status} label={healthReady ? "NOOP VERIFIED" : awaitingNightlyData ? "NIGHTLY SYNC PENDING" : health.status.replace(/_/g, " ")} /></header>
        {canShowHistoricalSignals && recoveryTrend.length > 0
          ? <Trace values={recoveryTrend} labels={recoveryLabels} tall tint="lime" />
          : <div className="health-signal-empty">
              <strong>Recovery baseline is still forming.</strong>
              <span>Sleep, HRV, and resting heart rate remain current while NOOP gathers more complete daily records.</span>
              {overnight && <div className="health-night-window" aria-label="Latest NOOP overnight sleep window">
                <div><span>Sleep began</span><time dateTime={overnight.startedAt}>{time(overnight.startedAt)}</time></div>
                <div className="health-night-span"><span>{sleepDurationLabel} asleep</span><i /></div>
                <div><span>Woke</span><time dateTime={overnight.endedAt}>{time(overnight.endedAt)}</time></div>
              </div>}
            </div>}
        <div className="health-signal-metrics">{secondarySignals.map((signal) => <span key={signal.label}>{signal.label}<strong>{signal.value}{signal.unit && <small>{signal.unit}</small>}</strong></span>)}</div>
      </section>

      <aside className="health-insights">
        <header><HeartPulse size={17} /><span>Current health context</span></header>
        {visibleInsights.slice(0, 3).map((insight) => <article key={insight.metric}><span>{insight.metric.replace(/_/g, " ")}</span><strong>{insight.fact}</strong>{insight.context && <p>{insight.context}</p>}</article>)}
        {visibleInsights.length === 0 && health.message && <p className="health-empty">{health.message}</p>}
      </aside>

      <section className="health-baseline">
        <div><span>Last night</span><strong>{sleepDurationLabel}</strong></div>
        <div><span>Sleep efficiency</span><strong>{number(overnightEfficiency == null ? null : overnightEfficiency * 100)}<small>%</small></strong></div>
        <div className="health-battery-summary"><span>Battery</span><div><strong>{number(health.battery?.percentage)}<small>%</small></strong><State value={health.battery?.charging ?? "unknown"} label={health.battery?.charging === true ? "CHARGING" : health.battery?.charging === false ? "ON STRAP" : "UNKNOWN"} /></div></div>
        <div><span>Data age</span><strong>{health.dataAgeDays == null ? "--" : `${health.dataAgeDays}d`}</strong></div>
      </section>

      <section className="health-history">{sleepStages.length > 0
        ? <><header><span>Sleep stages</span><small>{overnight?.sessionCount === 1 ? "Last night" : `${overnight?.sessionCount} merged sessions`}</small></header><div className="health-stage-bar" aria-label={sleepStages.map((stage) => `${stage.label} ${formatMinutes(stage.minutes)}`).join(", ")}>{sleepStages.map((stage) => <i key={stage.label} className={`health-stage--${stage.tone}`} style={{ flexGrow: stage.minutes }} />)}</div><div className="health-stage-labels">{sleepStages.map((stage) => <span key={stage.label}><i className={`health-stage--${stage.tone}`} />{stage.label}<strong>{formatMinutes(stage.minutes)}</strong></span>)}</div></>
        : <><header><span>HRV history</span></header>{canShowHistoricalSignals && hrvTrend.length > 0
          ? <Trace values={hrvTrend} labels={hrvLabels} />
          : <p className="health-history-empty">HRV history will appear after NOOP records multiple complete daily values.</p>}</>}</section>
    </div>
  );
}
