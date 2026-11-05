import { flushSync } from "react-dom";
import { reducedMotion } from "../shell/Splash";
import { screenFor, type Route } from "./routes";

/**
 * The route motion system.
 * - space: lens page to or from anything. The lens folds into the sidebar's Home item and back.
 * - section: section to section. Quick cross-fade with a 6px rise.
 * - subview: within a host. A 2px rise; the segmented control stays put.
 * - none: same screen, including anchor jumps such as Settings › Devices.
 */
export type RouteMotion = "none" | "space" | "section" | "subview";

export function routeMotion(from: Route, to: Route): RouteMotion {
  const fromScreen = screenFor(from), toScreen = screenFor(to);
  if (fromScreen === toScreen) return "none";
  if (fromScreen === "landing" || toScreen === "landing") return "space";
  return from.id === to.id ? "subview" : "section";
}

/** How the new view enters. The old view's exit is CSS: styles/route-motion.css. */
const ENTER = {
  forward: { transform: "scale(.988)", ms: 280 },
  back: { transform: "scale(1.012)", ms: 280 },
  section: { transform: "translateY(6px)", ms: 220 },
  subview: { transform: "translateY(2px)", ms: 180 },
} as const;

let entering: Animation[] = [];

/** The new view is live from its first frame and animates in place, so it takes clicks straight away. */
function playEnter(kind: Exclude<RouteMotion, "none">, back: boolean) {
  for (const animation of entering) animation.cancel();
  const { transform, ms } = ENTER[kind === "space" ? (back ? "back" : "forward") : kind];
  const field = document.querySelector<HTMLElement>(".active-field");
  const targets = field ? Array.from(field.children).filter((child) => kind !== "subview" || !child.classList.contains("subview-bar")) : [];
  entering = targets.flatMap((target) => target.animate?.(
    [{ opacity: 0, transform }, { opacity: 1, transform: "none" }],
    { duration: ms, easing: "cubic-bezier(.2, .8, .2, 1)" },
  ) ?? []);
}

let latest = 0;

/**
 * Apply a route change with its motion. `commit` always runs, so motion can never lose a navigation.
 *
 * Only the old view is captured: its snapshot fades out on top of the new view, which stays a real,
 * clickable page. A captured new view would swallow clicks until the transition ended. Starting
 * another transition skips this one to its end, which is what makes it interruptible. WebKit before
 * Safari 18 has no View Transitions, so there the new view just enters.
 */
export function transitionRoute(kind: RouteMotion, to: Route, commit: () => void) {
  if (kind === "none" || reducedMotion()) { commit(); return; }
  const back = screenFor(to) === "landing";
  if (typeof document.startViewTransition !== "function") {
    flushSync(commit);
    playEnter(kind, back);
    return;
  }
  const root = document.documentElement;
  const token = ++latest;
  root.dataset.routeMotion = kind;
  root.dataset.routeCapture = kind;
  root.dataset.routeDirection = back ? "back" : "forward";
  const transition = document.startViewTransition(() => {
    if (token === latest) delete root.dataset.routeCapture;
    flushSync(commit);
    playEnter(kind, back);
  });
  const settle = () => {
    if (token !== latest) return;
    delete root.dataset.routeMotion;
    delete root.dataset.routeDirection;
  };
  transition.ready.catch(() => { /* skipped: the commit still ran */ });
  // The markers last until the new view has finished entering too. A cancelled entry just means a newer route.
  void transition.finished.catch(() => undefined)
    .then(() => Promise.allSettled(entering.map((animation) => animation.finished)))
    .then(settle);
}
