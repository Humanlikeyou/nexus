import { useEffect, useState } from "react";
import { getHomeBrief, type HomeBrief, type HomeWeather } from "../bridge";

export const HOME_BRIEF_POLL_MS = 15 * 60_000;
/** Older than this is worse than nothing: it just isn't shown. */
export const WEATHER_MAX_AGE_MS = 3 * 60 * 60_000;

/** The brief itself goes stale too: a greeting written hours ago mustn't outlive its time of day. */
export function freshBrief(brief: HomeBrief | null, now: number): HomeBrief | null {
  if (!brief?.updatedAt) return null;
  const at = Date.parse(brief.updatedAt);
  return Number.isFinite(at) && now - at <= WEATHER_MAX_AGE_MS ? brief : null;
}

/** Weather worth showing: the backend can null any field, and a line needs at least a temperature or a word. */
export function usableWeather(weather: HomeWeather | null | undefined): HomeWeather | null {
  return weather && (weather.tempC != null || weather.summary) ? weather : null;
}

/** Amara's morning brief: checked at launch, when the window regains focus, and every 15 minutes. */
export function useHomeBrief(): HomeBrief | null {
  const [brief, setBrief] = useState<HomeBrief | null>(null);
  useEffect(() => {
    let current = true;
    const check = () => { getHomeBrief().then((next) => { if (current) setBrief(next); }, () => { /* no brief: the page just shows less */ }); };
    check();
    const timer = window.setInterval(check, HOME_BRIEF_POLL_MS);
    window.addEventListener("focus", check);
    return () => { current = false; window.clearInterval(timer); window.removeEventListener("focus", check); };
  }, []);
  return brief;
}
