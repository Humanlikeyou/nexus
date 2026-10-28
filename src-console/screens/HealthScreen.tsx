import { Activity, BatteryMedium, HeartPulse, Moon, RefreshCw, Sparkles, Wind } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import { INITIAL_SNAPSHOT } from "../bridge";
import { date, formatMinutes, number, time } from "../lib/format";
import type { NoopDailyMetric, NoopHealthInsight, NoopHealthSnapshot, SystemSnapshot } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, Meter, Ring, SegmentedControl, Sparkline, StatTile, type Tone } from "../ui";
import "./HealthScreen.css";

const NOOP_COMMAND = "get_noop_health_snapshot";
const DASH = "—";
/** bridge EMPTY_NOOP default; an internal placeholder, not a real device name. */
const EMPTY_DEVICE_ID = "my-whoop";

type Band = { word: "High" | "Medium" | "Low"; color: string; tone: Tone };

export function recoveryBand(value: number | null | undefined): Band | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  if (value >= 67) return { word: "High", color: "var(--data-recovery-high)", tone: "success" };
  if (value >= 34) return { word: "Medium", color: "var(--data-recovery-mid)", tone: "warning" };
  return { word: "Low", color: "var(--data-recovery-low)", tone: "danger" };
}

const STATUS_BADGE: Record<NoopHealthSnapshot["status"], { tone: Tone; text: string }> = {
  ready: { tone: "success", text: "Up to date" },
  stale: { tone: "warning", text: "Stale" },
  waiting_for_nightly_data: { tone: "warning", text: "Syncing overnight" },
  waiting_for_data: { tone: "neutral", text: "Waiting for data" },
  unavailable: { tone: "neutral", text: "No data" },
};

function hm(minutes: number | null | undefined) {
  if (typeof minutes !== "number" || !Number.isFinite(minutes)) return DASH;
  return `${Math.floor(minutes / 60)}h ${Math.round(minutes % 60)}m`;
}
function fmt(value: number | null | undefined, digits = 0) {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(digits) : DASH;
}
function signed(value: number | null | undefined) {
  if (typeof value !== "number" || !Number.isFinite(value)) return DASH;
  return `${value > 0 ? "+" : value < 0 ? "−" : "±"}${Math.abs(value).toFixed(1)}`;
}
function clock(iso: string | null | undefined) {
  if (!iso) return DASH;
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? DASH : d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" });
}
const pct = (fraction: number | null | undefined) => (typeof fraction === "number" && Number.isFinite(fraction) ? fraction * 100 : null);
const Missing = () => <span title="unavailable">{DASH}</span>;
const val = (text: string) => (text === DASH ? <Missing /> : text);

type TrendKey = "recovery" | "sleep" | "hrv" | "rhr" | "strain" | "skin" | "resp";
const TRENDS: Record<TrendKey, { label: string; unit: string; digits: number; color: string; pick: (d: NoopDailyMetric) => number | null }> = {
  recovery: { label: "Recovery", unit: "%", digits: 0, color: "var(--data-recovery-high)", pick: (d) => d.recovery },
  sleep: { label: "Sleep", unit: "h", digits: 1, color: "var(--data-sleep)", pick: (d) => (d.totalSleepMin == null ? null : d.totalSleepMin / 60) },
  hrv: { label: "HRV", unit: "ms", digits: 0, color: "var(--data-hrv)", pick: (d) => d.avgHrv },
  rhr: { label: "RHR", unit: "bpm", digits: 0, color: "var(--sys-red)", pick: (d) => d.restingHr },
  strain: { label: "Strain", unit: "", digits: 1, color: "var(--data-strain)", pick: (d) => (d.strain == null ? null : Math.min(21, Math.max(0, d.strain))) },
  skin: { label: "Skin temp", unit: "°C", digits: 1, color: "var(--sys-orange)", pick: (d) => d.skinTempDevC },
  resp: { label: "Resp", unit: "br/min", digits: 1, color: "var(--sys-teal)", pick: (d) => d.respRateBpm },
};

const INSIGHT_ICON: Array<[RegExp, LucideIcon]> = [
  [/sleep/, Moon], [/hrv|heart|overnight|rhr/, HeartPulse], [/battery/, BatteryMedium], [/resp/, Wind], [/recovery|strain/, Activity],
];
const insightIcon = (metric: string) => INSIGHT_ICON.find(([re]) => re.test(metric))?.[1] ?? Sparkles;

/** Server insights first, then the local fallbacks, deduped by metric. */
export function buildInsights(health: NoopHealthSnapshot): NoopHealthInsight[] {
  const today = health.today;
  const overnight = health.latestNight ?? health.latestSleep;
  const sleepMin = overnight?.durationMin ?? today?.totalSleepMin ?? null;
  const hrv = overnight?.avgHrv ?? today?.avgHrv ?? null;
  const rhr = overnight?.restingHr ?? today?.restingHr ?? null;
  const stages = [
    { label: "light", minutes: overnight?.lightMin ?? 0 },
    { label: "deep", minutes: overnight?.deepMin ?? 0 },
    { label: "REM", minutes: overnight?.remMin ?? 0 },
  ].filter((s) => s.minutes > 0);
  const out: NoopHealthInsight[] = [];
  if (sleepMin != null) {
    out.push({
      metric: "latest_sleep",
      fact: `${hm(sleepMin)} of sleep`,
      context: overnight ? `NOOP recorded the latest night from ${time(overnight.startedAt)} to ${time(overnight.endedAt)}.` : "Taken from the latest completed NOOP daily record.",
    });
  }
  if (stages.length > 0 && overnight) {
    out.push({
      metric: "sleep_stages",
      fact: stages.map((s) => `${formatMinutes(s.minutes)} ${s.label}`).join(", "),
      context: [
        overnight.sessionCount > 1 ? `Combined across ${overnight.sessionCount} sleep sessions.` : "From last night's sleep session.",
        overnight.awakeMin && overnight.awakeMin > 0 ? `${formatMinutes(overnight.awakeMin)} was staged awake within the sleep window.` : null,
      ].filter(Boolean).join(" "),
    });
  }
  if (hrv != null || rhr != null) {
    const readings = [hrv == null ? null : `HRV ${number(hrv)} ms`, rhr == null ? null : `resting heart rate ${number(rhr)} bpm`].filter(Boolean).join(", ");
    out.push({ metric: "overnight_signals", fact: readings.charAt(0).toUpperCase() + readings.slice(1), context: "These are measured overnight signals from the latest NOOP sleep record." });
  }
  if (health.battery) {
    out.push({
      // Internal id kept from the WHOOP era; user-facing copy says NOOP band.
      metric: "whoop_battery",
      fact: `NOOP band battery is ${number(health.battery.percentage)}%`,
      context: `${health.battery.charging === true ? "Charging" : health.battery.charging === false ? "On strap" : "Charge state unavailable"}; last observed ${date(health.battery.observedAt)}.`,
    });
  }
  const visible = [...health.insights];
  for (const insight of out) if (!visible.some((e) => e.metric === insight.metric)) visible.push(insight);
  return visible;
}

function BatteryBadge({ battery }: { battery: NoopHealthSnapshot["battery"] }) {
  if (!battery) return <Badge tone="neutral">NOOP band · no reading</Badge>;
  const state = battery.charging === true ? "Charging" : battery.charging === false ? "On strap" : "Charge state unknown";
  const tone: Tone = battery.percentage <= 15 && battery.charging !== true ? "danger" : "neutral";
  return <Badge tone={tone} title={`Observed ${date(battery.observedAt)}`}>NOOP band {number(battery.percentage)}% · {state}</Badge>;
}

export type HealthFieldProps = {
  data: SystemSnapshot;
  /** Re-runs the snapshot load (App's useSnapshot refresh). Retry is hidden without it. */
  onRetry?: () => void;
  /** Overrides the first-load detection (refreshedAt still equals INITIAL_SNAPSHOT's). */
  loading?: boolean;
};

export function HealthField({ data, onRetry, loading }: HealthFieldProps) {
  const health = data.noop;
  const isLoading = loading ?? data.refreshedAt === INITIAL_SNAPSHOT.refreshedAt;
  const failed = data.unavailableSources.some((s) => s.command === NOOP_COMMAND);

  if (isLoading) return <FitnessSkeleton />;

  const status = STATUS_BADGE[health.status];
  const header = (
    <header className="fitness__header">
      <div className="fitness__heading">
        <div className="fitness__title-row">
          <h1>Fitness</h1>
          {failed
            ? <Badge tone="danger">Unavailable</Badge>
            : health.status === "stale" && health.dataAgeDays != null
              ? <Badge tone="warning">Stale · {health.dataAgeDays}d</Badge>
              : <Badge tone={status.tone}>{status.text}</Badge>}
        </div>
        <p className="fitness__meta">
          {failed
            ? "NOOP health couldn't be read."
            : [health.generatedAt ? `Updated ${clock(health.generatedAt)}` : null, health.dataAgeDays != null ? `${health.dataAgeDays}d old` : null, health.deviceId && health.deviceId !== EMPTY_DEVICE_ID ? health.deviceId : null].filter(Boolean).join(" · ") || "Read directly from your local NOOP store."}
        </p>
      </div>
      <div className="fitness__header-trailing">
        {!failed && <BatteryBadge battery={health.battery} />}
        {failed && onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
      </div>
    </header>
  );

  if (failed || health.status === "unavailable" || health.status === "waiting_for_data") {
    return (
      <div className="fitness" data-state={failed ? "error" : "empty"}>
        {header}
        <Card className="fitness__empty" title={failed ? "NOOP health is unavailable" : "Waiting for the first NOOP record"} headingLevel={2}>
          <p className="fitness__callout">{failed ? "Nexus couldn't read the local NOOP store. Try again in a moment." : health.message || "NOOP health data is not available yet."}</p>
          <div className="fitness__tiles fitness__tiles--3">
            <StatTile label="Recovery" value={<Missing />} />
            <StatTile label="HRV" value={<Missing />} unit="ms" />
            <StatTile label="Sleep" value={<Missing />} />
          </div>
          <InsetGroup title="Readiness">
            <ListRow icon={HeartPulse} title="Data access" subtitle="Terrence has no NOOP daily record to read yet." value="Local store missing" />
            <ListRow icon={Sparkles} title="Morning briefing" subtitle="Amara stays quiet until a fresh sleep session is available." value="Waiting for nightly data" />
            <ListRow icon={BatteryMedium} title="Battery watch" subtitle={health.battery ? "A current NOOP band reading is available." : "No NOOP band battery reading is available locally."} value={health.battery ? `${number(health.battery.percentage)}%` : "Waiting for device state"} />
          </InsetGroup>
        </Card>
      </div>
    );
  }

  return <FitnessReady health={health} header={header} />;
}

function FitnessReady({ health, header }: { health: NoopHealthSnapshot; header: ReactNode }) {
  const [metric, setMetric] = useState<TrendKey>("recovery");
  const today = health.today;
  const nightly = health.status === "waiting_for_nightly_data";
  const overnight = health.latestNight ?? health.latestSleep;
  const sleepMin = overnight?.durationMin ?? today?.totalSleepMin ?? null;
  const hrv = overnight?.avgHrv ?? today?.avgHrv ?? null;
  const rhr = overnight?.restingHr ?? today?.restingHr ?? null;
  const eff = pct(overnight?.efficiency ?? today?.efficiency ?? null);
  const recovery = today?.recovery ?? null;
  const band = recoveryBand(recovery);
  const strain = today?.strain == null ? null : Math.min(21, Math.max(0, today.strain));
  const days = health.recentDays;
  const everNull = (pick: (d: NoopDailyMetric) => number | null) => days.every((d) => pick(d) == null) && (today == null || pick(today) == null);
  const pending = "Arrives after the sleep record closes";

  const stages = [
    { label: "Deep", value: overnight?.deepMin ?? today?.deepMin ?? 0, color: "var(--data-sleep)" },
    { label: "REM", value: overnight?.remMin ?? today?.remMin ?? 0, color: "color-mix(in srgb, var(--data-sleep) 65%, var(--bg-content))" },
    { label: "Light", value: overnight?.lightMin ?? today?.lightMin ?? 0, color: "color-mix(in srgb, var(--data-sleep) 35%, var(--bg-content))" },
  ];
  const nightStages = [...stages, { label: "Awake", value: overnight?.awakeMin ?? 0, color: "var(--fill-secondary)" }];

  const trendOptions = (Object.keys(TRENDS) as TrendKey[])
    .filter((k) => !((k === "skin" || k === "resp") && everNull(TRENDS[k].pick)))
    .map((k) => ({ value: k, label: TRENDS[k].label }));
  const trend = TRENDS[metric];
  const series = days.map(trend.pick);
  const points = series.filter((v): v is number => v != null && Number.isFinite(v));
  const mean = points.length ? points.reduce((a, b) => a + b, 0) / points.length : null;
  const f = (v: number) => `${v.toFixed(trend.digits)}${trend.unit && trend.unit !== "%" ? ` ${trend.unit}` : trend.unit}`;
  const dayLabels = days.length ? [days[0], days[Math.floor((days.length - 1) / 2)], days[days.length - 1]].map((d) => d.day.slice(5)) : [];
  const insights = buildInsights(health);

  return (
    <div className="fitness" data-state={health.status}>
      {header}
      {health.status === "stale" && <p className="fitness__callout" role="note">Showing the last NOOP record.</p>}

      <div className="fitness__row fitness__row--today">
        <Card title="Recovery" className="fitness__ring-card">
          {nightly
            ? <p className="fitness__pending">{pending}</p>
            : (
              <div className="fitness__ring-body">
                <Ring value={recovery} label="Recovery" size="lg" color={band?.color} valueText={band ? `${fmt(recovery)}%, ${band.word}` : undefined}>
                  <span className="fitness__ring-value">{recovery == null ? <Missing /> : `${fmt(recovery)}%`}</span>
                </Ring>
                <span className="fitness__band" data-band={band?.word.toLowerCase()}>{band ? band.word : "No recovery yet"}</span>
              </div>
            )}
        </Card>
        <Card title="Strain" className="fitness__ring-card">
          <div className="fitness__ring-body">
            <Ring value={strain} max={21} label="Strain" size="lg" color="var(--data-strain)" valueText={strain == null ? undefined : `${fmt(strain, 1)} of 21`}>
              <span className="fitness__ring-value">{strain == null ? <Missing /> : fmt(strain, 1)}</span>
              <span className="fitness__ring-unit">of 21</span>
            </Ring>
          </div>
        </Card>
        <Card title="Sleep" className="fitness__sleep-card">
          {nightly
            ? <p className="fitness__pending">{pending}</p>
            : (
              <div className="fitness__sleep-body">
                <p className="fitness__display">{val(hm(sleepMin))}</p>
                <div className="fitness__labelled-meter">
                  <span>Efficiency</span><span className="fitness__num">{eff == null ? <Missing /> : `${fmt(eff)}%`}</span>
                  <Meter value={eff} label="Sleep efficiency" color="var(--data-sleep)" />
                </div>
                <Meter label="Sleep stages" segments={stages} format={(v) => formatMinutes(v)} />
                <ul className="fitness__legend" aria-hidden="true">
                  {stages.map((s) => <li key={s.label}><i style={{ background: s.color }} />{s.label} <span className="fitness__num">{formatMinutes(s.value)}</span></li>)}
                </ul>
              </div>
            )}
        </Card>
      </div>

      <div className="fitness__tiles">
        <StatTile label="Resting HR" value={nightly ? <Missing /> : val(fmt(rhr))} unit="bpm" />
        <StatTile label="HRV" value={nightly ? <Missing /> : val(fmt(hrv))} unit="ms" />
        {!everNull((d) => d.spo2Pct) && <StatTile label="SpO₂" value={val(fmt(today?.spo2Pct, 1))} unit="%" />}
        {!everNull((d) => d.skinTempDevC) && <StatTile label="Skin temp" value={val(signed(today?.skinTempDevC))} unit="°C vs baseline" />}
        {!everNull((d) => d.respRateBpm) && <StatTile label="Respiratory" value={val(fmt(today?.respRateBpm, 1))} unit="br/min" />}
      </div>

      {!nightly && overnight && (
        <Card title="Last night" accessory={overnight.sessionCount > 1 ? <Badge tone="info">{overnight.sessionCount} sessions merged</Badge> : undefined}>
          <InsetGroup>
            <ListRow icon={Moon} title={<span className="fitness__num">{clock(overnight.startedAt)} → {clock(overnight.endedAt)}</span>} subtitle={overnight.day} />
          </InsetGroup>
          <Meter className="fitness__night-meter" label="Last night stages" segments={nightStages} format={(v) => formatMinutes(v)} />
          <ul className="fitness__legend" aria-hidden="true">
            {nightStages.map((s) => <li key={s.label}><i style={{ background: s.color }} />{s.label} <span className="fitness__num">{formatMinutes(s.value)}</span></li>)}
          </ul>
          <div className="fitness__tiles fitness__tiles--4">
            <StatTile label="Duration" value={val(hm(overnight.durationMin))} />
            <StatTile label="Efficiency" value={val(fmt(pct(overnight.efficiency)))} unit="%" />
            <StatTile label="RHR" value={val(fmt(overnight.restingHr))} unit="bpm" />
            <StatTile label="HRV" value={val(fmt(overnight.avgHrv))} unit="ms" />
          </div>
        </Card>
      )}

      {days.length > 0 && (
        <Card title="14-day trends">
          <SegmentedControl label="Trend metric" size="small" value={metric} options={trendOptions} onChange={setMetric} />
          {points.length < 2
            ? <p className="fitness__pending">Trend appears after 2+ days of data.</p>
            : (
              <figure className="fitness__trend">
                <Sparkline values={series} label={`${trend.label}, last ${days.length} days`} format={f} showMean size="lg" color={trend.color} />
                <div className="fitness__axis" aria-hidden="true">{dayLabels.map((d, i) => <span key={`${d}-${i}`}>{d}</span>)}</div>
                <figcaption className="fitness__caption">Mean {f(mean as number)} · Min {f(Math.min(...points))} · Max {f(Math.max(...points))}</figcaption>
              </figure>
            )}
        </Card>
      )}

      {insights.length > 0 && (
        <InsetGroup title="Insights">
          {insights.map((i) => <ListRow key={i.metric} icon={insightIcon(i.metric)} title={i.fact} subtitle={i.context ?? undefined} />)}
        </InsetGroup>
      )}
      {insights.length === 0 && <p className="fitness__pending">Insights appear after NOOP&apos;s nightly sync.</p>}
    </div>
  );
}

function FitnessSkeleton() {
  return (
    <div className="fitness" data-state="loading" aria-busy="true" aria-label="Loading Fitness">
      <header className="fitness__header"><div className="fitness__heading"><h1>Fitness</h1><span className="fitness__skel fitness__skel--line" /></div></header>
      <div className="fitness__row fitness__row--today">
        {["Recovery", "Strain", "Sleep"].map((t) => (
          <Card key={t} title={t} className="fitness__ring-card"><div className="fitness__ring-body"><span className="fitness__skel fitness__skel--ring" /></div></Card>
        ))}
      </div>
      <div className="fitness__tiles">
        {[0, 1, 2, 3, 4].map((i) => <span key={i} className="fitness__skel fitness__skel--tile" />)}
      </div>
    </div>
  );
}
