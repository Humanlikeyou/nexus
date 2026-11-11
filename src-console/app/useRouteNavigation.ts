import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { preloadAllScreens, preloadScreen } from "./lazyScreens";
import { routeMotion, transitionRoute } from "./routeMotion";
import { canonicalRoute, screenFor, type Route } from "./routes";

/** A slow chunk gets this long to arrive before the page moves anyway and shows its skeleton. */
const CHUNK_WAIT_MS = 160;
/** Frames to wait for a lazy screen's heading, about 1.5s. */
const HEADING_TRIES = 90;

const fieldOf = () => document.querySelector<HTMLElement>(".active-field");

/** Move focus to the new page's heading, unless the page or its sub-view control already holds it. */
function focusHeading(field: HTMLElement): boolean {
  const heading = field.querySelector<HTMLElement>("h1");
  if (!heading) return false;
  const focused = document.activeElement;
  if (focused && focused !== document.body && field.contains(focused)) return true;
  if (!heading.hasAttribute("tabindex")) heading.tabIndex = -1;
  heading.focus({ preventScroll: true });
  // focus() is a no-op while the element can't take it yet (mid view-transition, not yet rendered): report whether it landed.
  return document.activeElement === heading;
}

/** The app's route, with one `navigate` for every link, tab, shortcut and ⌘K jump. */
export function useRouteNavigation(initial: Route) {
  const [route, setRoute] = useState(initial);
  const committed = useRef(initial);
  const request = useRef(0);

  const navigate = useCallback((target: Route) => {
    // design-1111v: every old route (business:*, terrance:goals, …) lands on where it lives now.
    const to = canonicalRoute(target);
    const id = ++request.current;
    const go = () => {
      if (id !== request.current) return; // a newer click won
      const from = committed.current;
      committed.current = to;
      // A skipped transition may run its update late, so every commit applies the newest route.
      transitionRoute(routeMotion(from, to), to, () => setRoute(committed.current));
    };
    const chunk = preloadScreen(screenFor(to));
    if (!chunk) { go(); return; }
    void Promise.race([chunk, new Promise((resolve) => window.setTimeout(resolve, CHUNK_WAIT_MS))]).then(go);
  }, []);

  // Screens load in the background after launch, so a click never waits for a chunk.
  useEffect(() => preloadAllScreens(), []);

  const screen = screenFor(route);
  // The pane outlives its screens: a new screen starts at the top, before the transition captures it.
  useLayoutEffect(() => { fieldOf()?.scrollTo?.({ top: 0 }); }, [screen]);

  const settled = useRef(false);
  useEffect(() => {
    if (!settled.current) { settled.current = true; return; } // launch keeps its natural focus
    let frame = 0, tries = 0;
    const attempt = () => {
      const field = fieldOf();
      if (field && focusHeading(field)) return;
      if (tries++ < HEADING_TRIES) frame = requestAnimationFrame(attempt);
    };
    frame = requestAnimationFrame(attempt);
    return () => cancelAnimationFrame(frame);
  }, [screen]);

  return { route, navigate };
}
