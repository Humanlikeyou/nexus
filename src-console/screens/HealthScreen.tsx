import { Activity, BatteryCharging, BatteryLow, BatteryMedium, HeartPulse, Inbox, Moon, RefreshCw, Sparkles, Wind } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useState, type ReactNode } from "react";
import { INITIAL_SNAPSHOT } from "../bridge";
import { EMPTY_DEVICE_ID } from "../lib/devices";
import { date, formatMinutes, number } from "../lib/format";
import type { NoopDailyMetric, NoopHealthInsight, NoopHealthSnapshot, SystemSnapshot } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, Meter, Ring, SegmentedControl, Sparkline, StatTile, type Tone } from "../ui";
import "./HealthScreen.css";

const NOOP_COMMAND = "get_noop_health_snapshot";
const DASH = "—";
/** bridge EMPTY_NOOP default; an internal placeholder, not a real device name. */

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
export function signed(value: number | null | undefined) {
  if (typeof value !== "number" || !Number.isFinite(value)) return DASH;
  // Zero is "0.0" (no sign); ± would read as a tolerance. U+2212 for negatives.
  const text = Math.abs(value).toFixed(1);
  if (Number(text) === 0) return "0.0";
  return `${value > 0 ? "+" : "−"}${text}`;
}
function clock(iso: string | null | undefined) {
  if (!iso) return DASH;
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? DASH : d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false, timeZone: "Asia/Tokyo" });
}
const pct = (fraction: number | null | undefined) => (typeof fraction === "number" && Number.isFinite(fraction) ? fraction * 100 : null);
/** "2026-09-20" → "Sep 20" (a calendar day, not an instant: no time, no UTC shift). */
export function dayLabel(day: string): string {
  const parsed = new Date(`${day}T00:00:00`);
  return Number.isNaN(parsed.getTime()) ? day : date(parsed);
}
/** Fitness keeps 24-hour times: "Oct 28, 13:38". */
const observed = (iso: string | null | undefined) => (iso ? `${date(iso)}, ${clock(iso)}` : "No record");

const Missing = () => <span title="unavailable">{DASH}</span>;
const val = (text: string) => (text === DASH ? <Missing /> : text);
/** StatTile renders null as a tertiary "—" and hides the unit. */
const nullable = (text: string) => (text === DASH ? null : text);

/** NOOP stores daily strain as "Effort" on its native 0–100 axis (StrainScorer.maxStrain = 100). */
const EFFORT_MAX = 100;
const clampEffort = (v: number) => Math.min(EFFORT_MAX, Math.max(0, v));

type TrendKey = "recovery" | "sleep" | "hrv" | "rhr" | "strain" | "skin" | "resp";
const TRENDS: Record<TrendKey, { label: string; unit: string; digits: number; color: string; pick: (d: NoopDailyMetric) => number | null }> = {
  recovery: { label: "Recovery", unit: "%", digits: 0, color: "var(--data-recovery-high)", pick: (d) => d.recovery },
  sleep: { label: "Sleep", unit: "h", digits: 1, color: "var(--data-sleep)", pick: (d) => (d.totalSleepMin == null ? null : d.totalSleepMin / 60) },
  hrv: { label: "HRV", unit: "ms", digits: 0, color: "var(--data-hrv)", pick: (d) => d.avgHrv },
  rhr: { label: "RHR", unit: "bpm", digits: 0, color: "var(--sys-red)", pick: (d) => d.restingHr },
  strain: { label: "Effort", unit: "", digits: 0, color: "var(--data-strain)", pick: (d) => (d.strain == null ? null : clampEffort(d.strain)) },
  skin: { label: "Skin temp", unit: "°C", digits: 1, color: "var(--sys-orange)", pick: (d) => d.skinTempDevC },
  resp: { label: "Resp", unit: "br/min", digits: 1, color: "var(--sys-teal)", pick: (d) => d.respRateBpm },
};

const INSIGHT_ICON: Array<[RegExp, LucideIcon]> = [
  [/sleep/, Moon], [/hrv|heart|overnight|rhr/, HeartPulse], [/battery/, BatteryMedium], [/resp/, Wind], [/recovery|strain|effort/, Activity],
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
      context: overnight ? `NOOP recorded the latest night from ${clock(overnight.startedAt)} to ${clock(overnight.endedAt)}.` : "Taken from the latest completed NOOP daily record.",
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
      context: `${health.battery.charging === true ? "Charging" : health.battery.charging === false ? "On strap" : "Charge state unavailable"}; last observed ${observed(health.battery.observedAt)}.`,
    });
  }
  const visible = [...health.insights];
  for (const insight of out) if (!visible.some((e) => e.metric === insight.metric)) visible.push(insight);
  return visible;
}

/** Sentence-case status text with a battery glyph (badges are for 1–2 word tags). */
function BatteryStatus({ battery }: { battery: NoopHealthSnapshot["battery"] }) {
  if (!battery) return <span className="fitness__status"><BatteryMedium className="fitness__status-icon" aria-hidden="true" />NOOP band · no reading</span>;
  const state = battery.charging === true ? "Charging" : battery.charging === false ? "On strap" : "Charge state unknown";
  const low = battery.percentage <= 15 && battery.charging !== true;
  const Icon = battery.charging === true ? BatteryCharging : low ? BatteryLow : BatteryMedium;
  return (
    <span className="fitness__status" data-tone={low ? "danger" : undefined} title={`Observed ${observed(battery.observedAt)}`}>
      <Icon className="fitness__status-icon" aria-hidden="true" />NOOP band {number(battery.percentage)}% · {state}
    </span>
  );
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
            : <Badge tone={status.tone}>{status.text}</Badge>}
        </div>
        <p className="fitness__meta">
          {failed
            ? "NOOP health couldn't be read."
            : [health.generatedAt ? `Updated ${clock(health.generatedAt)}` : null, health.dataAgeDays != null ? `${health.dataAgeDays}d old` : null, health.deviceId && health.deviceId !== EMPTY_DEVICE_ID ? health.deviceId : null].filter(Boolean).join(" · ") || "Read directly from your local NOOP store."}
        </p>
      </div>
      <div className="fitness__header-trailing">
        {!failed && <BatteryStatus battery={health.battery} />}
        {failed && onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
      </div>
    </header>
  );

  if (failed || health.status === "unavailable" || health.status === "waiting_for_data") {
    return (
      <div className="fitness" data-state={failed ? "error" : "empty"}>
        {header}
        <Card className="fitness__empty" title={failed ? "NOOP health is unavailable" : "Waiting for the first NOOP record"} headingLevel={2}>
          <div className="fitness__empty-lead">
            <Inbox className="fitness__empty-icon" aria-hidden="true" />
            <p className="fitness__callout">{failed ? "Amara OS couldn't read the local NOOP store. Try again in a moment." : health.message || "NOOP health data is not available yet."}</p>
            {!failed && onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Check Again</Button>}
          </div>
          <div className="fitness__tiles fitness__tiles--3">
            <StatTile label="Recovery" value={null} />
            <StatTile label="HRV" value={null} unit="ms" />
            <StatTile label="Sleep" value={null} />
          </div>
          <InsetGroup title="Readiness" className="fitness__readiness">
            <ListRow icon={HeartPulse} title="Data access" subtitle="Amara has no NOOP daily record to read yet." accessory={<Badge tone="neutral">Store missing</Badge>} />
            <ListRow icon={Sparkles} title="Morning briefing" subtitle="Amara stays quiet until a fresh sleep session is available." accessory={<Badge tone="neutral">Waiting</Badge>} />
            <ListRow icon={BatteryMedium} title="Battery watch" subtitle={health.battery ? "A current NOOP band reading is available." : "No NOOP band battery reading is available locally."} accessory={<Badge tone="neutral">{health.battery ? `${number(health.battery.percentage)}%` : "No reading"}</Badge>} />
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
  const strain = today?.strain == null ? null : clampEffort(today.strain);
  const days = health.recentDays;
  const everNull = (pick: (d: NoopDailyMetric) => number | null) => days.every((d) => pick(d) == null) && (today == null || pick(today) == null);
  const pending = "Arrives after the sleep record closes";
  const extraTiles = [
    !everNull((d) => d.spo2Pct) && <StatTile key="spo2" label="SpO₂" value={nullable(fmt(today?.spo2Pct, 1))} unit="%" />,
    !everNull((d) => d.skinTempDevC) && <StatTile key="skin" label="Skin temp" value={nullable(signed(today?.skinTempDevC))} unit="°C vs baseline" />,
    !everNull((d) => d.respRateBpm) && <StatTile key="resp" label="Respiratory" value={nullable(fmt(today?.respRateBpm, 1))} unit="br/min" />,
  ].filter(Boolean);

  const stages = [
    { label: "Deep", value: overnight?.deepMin ?? today?.deepMin ?? 0, color: "var(--data-sleep-deep)" },
    { label: "REM", value: overnight?.remMin ?? today?.remMin ?? 0, color: "var(--data-sleep-rem)" },
    { label: "Light", value: overnight?.lightMin ?? today?.lightMin ?? 0, color: "var(--data-sleep-light)" },
  ];
  const nightStages = [...stages, { label: "Awake", value: overnight?.awakeMin ?? 0, color: "var(--data-sleep-awake)" }];

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
      {health.status === "stale" && (
        <p className="fitness__callout" role="note">
          Last NOOP data {health.latestDay ? dayLabel(health.latestDay) : "unknown"}. <span className="fitness__hint">Open NOOP to sync.</span>
        </p>
      )}

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
        <Card title="Effort" className="fitness__ring-card">
          <div className="fitness__ring-body">
            <Ring value={strain} max={EFFORT_MAX} label="Effort" size="lg" color="var(--data-strain)" valueText={strain == null ? undefined : `${fmt(strain)} of ${EFFORT_MAX}`}>
              <span className="fitness__ring-value">{strain == null ? <Missing /> : fmt(strain)}</span>
              <span className="fitness__ring-unit">of {EFFORT_MAX}</span>
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

      {/* Row 2: only signals not shown elsewhere (RHR/HRV live in Last night); hidden when all are null. */}
      {extraTiles.length > 0 && <div className="fitness__tiles">{extraTiles}</div>}

      {!nightly && overnight && (
        <Card title="Last night" accessory={overnight.sessionCount > 1 ? <Badge tone="info">{overnight.sessionCount} sessions merged</Badge> : undefined}>
          <InsetGroup>
            <ListRow icon={Moon} title={<span className="fitness__num">{clock(overnight.startedAt)} → {clock(overnight.endedAt)}</span>} subtitle={overnight.day} />
          </InsetGroup>
          <Meter className="fitness__night-meter" label="Last night stages" segments={nightStages} format={(v) => formatMinutes(v)} />
          <ul className="fitness__legend" aria-hidden="true">
            {nightStages.map((s) => <li key={s.label}><i style={{ background: s.color }} />{s.label} <span className="fitness__num">{formatMinutes(s.value)}</span></li>)}
          </ul>
          {/* Duration and efficiency already lead the Sleep card; keep the overnight signals only, at Title1. */}
          <div className="fitness__tiles fitness__tiles--2 fitness__tiles--compact">
            <StatTile label="Resting HR" value={nullable(fmt(overnight.restingHr ?? rhr))} unit="bpm" />
            <StatTile label="HRV" value={nullable(fmt(overnight.avgHrv ?? hrv))} unit="ms" />
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
        {["Recovery", "Effort", "Sleep"].map((t) => (
          <Card key={t} title={t} className="fitness__ring-card"><div className="fitness__ring-body"><span className="fitness__skel fitness__skel--ring" /></div></Card>
        ))}
      </div>
      <div className="fitness__tiles">
        {[0, 1, 2, 3, 4].map((i) => <span key={i} className="fitness__skel fitness__skel--tile" />)}
      </div>
    </div>
  );
}
