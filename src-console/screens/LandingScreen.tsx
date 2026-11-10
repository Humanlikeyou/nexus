import { Heart } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { LANDING_ENTRIES } from "../app/routes";
import { AmaraLens, AmaraVoiceAnnouncer } from "../components/AmaraLens/AmaraLens";
import { freshBrief, usableWeather, useHomeBrief } from "../hooks/useHomeBrief";
import { useLandingVoice } from "../hooks/useLandingVoice";
import { beatSeconds, useLiveHeartRate } from "../hooks/useLiveHeartRate";
import { localGreeting } from "../lib/greeting";
import { heartSlots } from "../lib/heartTrace";
import { TrendChart } from "../ui/charts/TrendChart";
import { useNow } from "../shell/useNow";
import type { ModuleId } from "../types";
import { Wordmark } from "../ui/Wordmark";
import "./LandingScreen.css";

const formatTime = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", hour12: true });
const formatDate = new Intl.DateTimeFormat("en-US", { weekday: "long", month: "long", day: "numeric" });

/** Usual resting heart rate from NOOP's daily rows (null = hide the band, the line and its legend). */
type Resting = { low: number; high: number; median: number } | null;

/**
 * Live heart rate from NOOP, with the last 100 seconds behind it (design-1110c C14): an ember 5-s
 * average with a soft fill, 40/70/100 bpm ticks, −99 s / −50 s / now, a hover readout, and his
 * resting band from the real daily resting HR. With no live reading it's all gone.
 */
function HeartRate({ resting = null }: { resting?: Resting }) {
  const heart = useLiveHeartRate();
  const beat = beatSeconds(heart?.bpm);
  if (!heart || beat == null) return null;
  const slots = heartSlots(heart.samples, heart.at);
  const n = slots.length;
  const top = Math.max(100, ...slots.filter((v): v is number => v != null));
  const domain: [number, number] = [40, top > 100 ? Math.ceil(top / 10) * 10 : 100];
  return (
    <div className="landing__heart-rate" role="group" aria-label={`Heart rate ${heart.bpm} beats per minute`}>
      <p className="landing__pulse">
        <span className="landing__heart" style={{ "--landing-beat": `${beat}s` } as CSSProperties}><Heart size={13} fill="currentColor" aria-hidden="true" /></span>
        <span className="landing__bpm">{heart.bpm}</span>
        <span className="landing__weather-detail">bpm</span>
      </p>
      <div className="landing__hr-chart">
        <TrendChart values={slots} labels={slots.map((_, i) => `\u2212${n - 1 - i} s`)} lastLabel="now" tone="ember" unit="bpm" domain={domain}
          ticks={domain[1] === 100 ? [40, 70, 100] : undefined} xTicks={[0, 49, n - 1]} smoothing={5} rawDots={false} height={140} usual={resting}
          label={`Heart rate, last ${n} seconds. Now ${heart.bpm} bpm.${resting ? ` Resting usual ${Math.round(resting.median)} bpm.` : ""}`}
          readout={(i) => <><strong>{slots[i]} bpm</strong><span>{n - 1 - i} s ago</span></>} />
        <p className="chart-legend landing__hr-legend">
          <span><i aria-hidden="true" />Live · 5-s average</span>
          {resting && <span><i data-key="usual" aria-hidden="true" />{`Resting usual ${Math.round(resting.median)} bpm`}</span>}
        </p>
      </div>
    </div>
  );
}

/** Greeting, time, date and weather. Nothing here is a box; the type scale does the work. */
function Welcome({ resting = null }: { resting?: Resting }) {
  const now = useNow(1_000);
  const brief = freshBrief(useHomeBrief(), now.getTime());
  const weather = usableWeather(brief?.weather);
  const extra = weather && [weather.place, weather.highC != null ? `High ${Math.round(weather.highC)}°` : null, weather.lowC != null ? `Low ${Math.round(weather.lowC)}°` : null].filter(Boolean).join(" · ");
  const summary = weather?.summary ? weather.summary[0].toUpperCase() + weather.summary.slice(1) : null;
  return (
    <section className="landing__welcome" aria-label="Today">
      <p className="landing__greeting">{brief?.greeting || localGreeting(now)}</p>
      <time className="landing__time" dateTime={now.toISOString()}>{formatTime.format(now)}</time>
      <p className="landing__date">{formatDate.format(now)}</p>
      {weather && (
        <p className="landing__weather">
          <span>{weather.tempC != null && <span className="landing__temp">{Math.round(weather.tempC)}° </span>}{summary}</span>
          {extra && <span className="landing__weather-detail">{extra}</span>}
        </p>
      )}
      <HeartRate resting={resting} />
    </section>
  );
}

/**
 * Home: the screen the app opens on. The lens shows Amara's real voice state and the buttons below
 * it stand in for the sidebar. Always dark in both themes: the lens is an object and its glow only
 * reads against the emerald field (LandingScreen.css).
 */
export function LandingField({ onSelect, onToday, chat, chatExpanded = false, resting = null }: { onSelect: (id: ModuleId) => void; onToday: () => void; chat?: ReactNode; chatExpanded?: boolean; resting?: Resting }) {
  const voice = useLandingVoice();
  return (
    <div className="landing" data-voice={voice.state} data-chat={chat ? (chatExpanded ? "expanded" : "collapsed") : undefined}>
      <div className="landing__drag" data-tauri-drag-region />
      <h1 className="ui-visually-hidden">Command Center</h1>
      <Wordmark size="landing" className="landing__wordmark" />
      <Welcome resting={resting} />
      <div className="landing__stage">
      <div className="landing__lens"><AmaraLens state={voice.state} size={320} decorative /></div>
      <p className="landing__line">{voice.line}</p>
      <AmaraVoiceAnnouncer message={voice.line} />
      {/* design-1110b A: the chat lives here, directly under the lens. */}
      {chat}
      <nav className="landing__nav" aria-label="Amara OS navigation">
        {LANDING_ENTRIES.map((entry) => {
          const Icon = entry.icon;
          return (
            <button key={entry.id} type="button" className="landing__button" title={entry.shortcut ? `${entry.label} (Command+${entry.shortcut})` : entry.label}
              onClick={() => { if (entry.id === "terrance") onToday(); else onSelect(entry.id); }}>
              <Icon size={17} aria-hidden="true" /><span>{entry.label}</span>
            </button>
          );
        })}
      </nav>
      </div>
    </div>
  );
}
