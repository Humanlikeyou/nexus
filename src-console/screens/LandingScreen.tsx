import { LANDING_ENTRIES } from "../app/routes";
import { AmaraLens, AmaraVoiceAnnouncer } from "../components/AmaraLens/AmaraLens";
import { useLandingVoice } from "../hooks/useLandingVoice";
import type { ModuleId } from "../types";
import { Wordmark } from "../ui/Wordmark";
import "./LandingScreen.css";

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
  );
}
