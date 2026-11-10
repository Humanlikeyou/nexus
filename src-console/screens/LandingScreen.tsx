import { Heart } from "lucide-react";
import type { CSSProperties } from "react";
import { LANDING_ENTRIES } from "../app/routes";
import { AmaraLens, AmaraVoiceAnnouncer } from "../components/AmaraLens/AmaraLens";
import { freshBrief, usableWeather, useHomeBrief } from "../hooks/useHomeBrief";
import { useLandingVoice } from "../hooks/useLandingVoice";
import { beatSeconds, useLiveHeartRate } from "../hooks/useLiveHeartRate";
import { localGreeting } from "../lib/greeting";
import { TRACE_HEIGHT, TRACE_WIDTH, traceShape } from "../lib/heartTrace";
import { useNow } from "../shell/useNow";
import type { ModuleId } from "../types";
import { Wordmark } from "../ui/Wordmark";
import "./LandingScreen.css";

const formatTime = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", hour12: true });
const formatDate = new Intl.DateTimeFormat("en-US", { weekday: "long", month: "long", day: "numeric" });

/** Live heart rate from NOOP, with the last 89 seconds behind it. With no live reading it's all gone. */
function HeartRate() {
  const heart = useLiveHeartRate();
  const beat = beatSeconds(heart?.bpm);
  if (!heart || beat == null) return null;
  const { paths, dot } = traceShape(heart.samples, heart.at);
  return (
    <div className="landing__heart-rate" role="img" aria-label={`Heart rate ${heart.bpm} beats per minute`}>
      <p className="landing__pulse">
        <span className="landing__heart" style={{ "--landing-beat": `${beat}s` } as CSSProperties}><Heart size={13} fill="currentColor" aria-hidden="true" /></span>
        <span className="landing__bpm">{heart.bpm}</span>
        <span className="landing__weather-detail">bpm</span>
      </p>
      {dot && (
        <svg className="landing__trace" width={TRACE_WIDTH} height={TRACE_HEIGHT} viewBox={`0 0 ${TRACE_WIDTH} ${TRACE_HEIGHT}`} aria-hidden="true">
          {paths.map((path) => <path key={path} className="landing__trace-line" d={path} />)}
          <circle className="landing__trace-halo" cx={dot.x} cy={dot.y} r="8" />
          <circle className="landing__trace-dot" cx={dot.x} cy={dot.y} r="3" />
        </svg>
      )}
    </div>
  );
}

/** Greeting, time, date and weather. Nothing here is a box; the type scale does the work. */
function Welcome() {
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
      <HeartRate />
    </section>
  );
}

/**
 * Home: the screen the app opens on. The lens shows Amara's real voice state and the buttons below
 * it stand in for the sidebar. Always dark in both themes: the lens is an object and its glow only
 * reads against the emerald field (LandingScreen.css).
 */
export function LandingField({ onSelect, onToday }: { onSelect: (id: ModuleId) => void; onToday: () => void }) {
  const voice = useLandingVoice();
  return (
    <div className="landing" data-voice={voice.state}>
      <div className="landing__drag" data-tauri-drag-region />
      <h1 className="ui-visually-hidden">Command Center</h1>
      <Wordmark size="landing" className="landing__wordmark" />
      <Welcome />
      <div className="landing__stage">
      <div className="landing__lens"><AmaraLens state={voice.state} size={320} decorative /></div>
      <p className="landing__line">{voice.line}</p>
      <AmaraVoiceAnnouncer message={voice.line} />
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
