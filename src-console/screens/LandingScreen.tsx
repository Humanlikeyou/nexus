import { Heart } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { LANDING_ENTRIES } from "../app/routes";
import { AmaraVoiceAnnouncer } from "../components/AmaraLens/AmaraLens";
import { SandLensLayer } from "../sandLens/SandLens";
import { freshBrief, usableWeather, useHomeBrief } from "../hooks/useHomeBrief";
import { useLandingVoice } from "../hooks/useLandingVoice";
import { beatSeconds, useLiveHeartRate } from "../hooks/useLiveHeartRate";
import { localGreeting } from "../lib/greeting";
import { TRACE_HEIGHT, TRACE_WIDTH, traceShape } from "../lib/heartTrace";
import { useNow } from "../shell/useNow";
import { HrTake } from "../hrTake/HrTakeCard";
import { useTakeSlots } from "../takes/TakeExtras";
import type { ModuleId } from "../types";
import { Wordmark } from "../ui/Wordmark";
import { HomeWindowsStage, landingInert } from "../homeWindows/HomeWindows";
import { useHomeWindowsState } from "../homeWindows/store";
import "./LandingScreen.css";

const formatTime = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", hour12: true });
const formatDate = new Intl.DateTimeFormat("en-US", { weekday: "long", month: "long", day: "numeric" });

/** Live heart rate from NOOP, with the last 89 seconds behind it. With no live reading it's all gone. */
function HeartRate() {
  const heart = useLiveHeartRate();
  const beat = beatSeconds(heart?.bpm);
  if (!heart || beat == null) return null;
  const { paths, areas = [], dot } = traceShape(heart.samples, heart.at);
  return (
    <div className="landing__heart-rate" role="img" aria-label={`Heart rate ${heart.bpm} beats per minute`}>
      <p className="landing__pulse">
        <span className="landing__heart" style={{ "--landing-beat": `${beat}s` } as CSSProperties}><Heart size={13} fill="currentColor" aria-hidden="true" /></span>
        <span className="landing__bpm">{heart.bpm}</span>
        <span className="landing__weather-detail">bpm</span>
      </p>
      {dot && (
        <svg className="landing__trace" width={TRACE_WIDTH} height={TRACE_HEIGHT} viewBox={`0 0 ${TRACE_WIDTH} ${TRACE_HEIGHT}`} aria-hidden="true">
          <defs><linearGradient id="landing-trace-fill" x1="0" y1="0" x2="0" y2="1"><stop offset="0" className="landing__trace-stop0" /><stop offset="1" className="landing__trace-stop1" /></linearGradient></defs>
          {areas.map((area) => <path key={area} className="landing__trace-area" d={area} fill="url(#landing-trace-fill)" />)}
          {paths.map((path) => <path key={path} className="landing__trace-line" d={path} />)}
          <circle className="landing__trace-halo" cx={dot.x} cy={dot.y} r="8" />
          <circle className="landing__trace-dot" cx={dot.x} cy={dot.y} r="3" />
        </svg>
      )}
    </div>
  );
}

/** design-1111u: Amara's take from the last 15 minutes of heart rate, with today's one thing (1111t) under it.
 *  No button: it refreshes on open, focus and navigation, and the backend's 30-minute rule decides.
 *  Taisun, 2026-11-12 (late): removing it from Home and Today in 686340d was a mistake; it is back on both. */
function HomeTake({ onSelect }: { onSelect: (id: ModuleId) => void }) {
  const { block } = useTakeSlots("home", true);
  return <HrTake variant="landing" block={block} onOpenDevices={() => onSelect("devices")} />;
}

/** Greeting, time, date and weather. Nothing here is a box; the type scale does the work. */
function Welcome({ onSelect }: { onSelect: (id: ModuleId) => void }) {
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
      <HomeTake onSelect={onSelect} />
    </section>
  );
}

/**
 * Home: the screen the app opens on. The lens shows Amara's real voice state and the buttons below
 * it stand in for the sidebar. Always dark in both themes: the lens is an object and its glow only
 * reads against the emerald field (LandingScreen.css).
 */
export function LandingField({ onSelect, onToday, chat, chatExpanded = false, onAsk }: { onSelect: (id: ModuleId) => void; onToday: () => void; chat?: ReactNode; chatExpanded?: boolean; onAsk?: (text: string) => void }) {
  const voice = useLandingVoice();
  const windows = useHomeWindowsState();
  const hide = landingInert(windows.phase);
  return (
    <div className="landing" data-voice={voice.state} data-wphase={windows.phase} data-chat={chat ? (chatExpanded ? "expanded" : "collapsed") : undefined}>
      <div className="landing__drag" data-tauri-drag-region />
      <h1 className="ui-visually-hidden">Command Center</h1>
      <Wordmark size="landing" className="landing__wordmark" />
      <div className="landing__side" inert={hide || undefined} aria-hidden={hide || undefined}>
      <Welcome onSelect={onSelect} />
      </div>
      <div className="landing__stage" inert={hide || undefined} aria-hidden={hide || undefined}>
      <div className="landing__lens"><SandLensLayer voice={voice.state} line={voice.line} active={!hide} /></div>
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
      <HomeWindowsStage onAsk={onAsk} />
    </div>
  );
}
