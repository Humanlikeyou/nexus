/** design-1111x: hidden, nameless, session-only dark look. No Settings toggle. Does not persist. */

export type NavySource = "voice" | "chat" | "hold" | "preview";
export type NavyPhase = "off" | "entering" | "on" | "exiting";
export type NavyHold = "idle" | "holding" | "confirm";

export type NavyCaption = { text: string; sample: boolean; spoken: boolean };

export type NavyState = {
  phase: NavyPhase;
  /** Who turned it on. Null when off. */
  openedBy: NavySource | null;
  caption: NavyCaption | null;
  hold: NavyHold;
  /** 0–1 while holding the orb. */
  holdAt: number;
};

export const initialNavyState = (): NavyState => ({
  phase: "off", openedBy: null, caption: null, hold: "idle", holdAt: 0,
});

/** Mid-frame / test trigger only. Not the live on/off flag — that never goes to disk. */
export const NAVY_PREVIEW_KEY = "nexus.navyPreview";
export type NavyPreviewScenario = "on" | "off" | "tear" | "flash" | "cut" | "holding" | "confirm";
export type NavyPreview = { scenario: NavyPreviewScenario; openedBy?: NavySource };
