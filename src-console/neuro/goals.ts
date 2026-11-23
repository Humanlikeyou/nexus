import type { NfProtocol } from "../legacy-core/lib/types";

/** The eight protocols, grouped by what he wants from a session. */
export type Goal = { id: string; title: string; line: string; protocolIds: string[]; alpha: boolean };

export const GOALS: Goal[] = [
  { id: "settle", title: "Settle", line: "Calm the mind and let the body soften.", protocolIds: ["calm", "relaxed-flow"], alpha: true },
  { id: "deep", title: "Go deep", line: "Eyes closed, led by sound and the feeling of the session.", protocolIds: ["deep-relaxation", "deep-meditation", "hypnagogic"], alpha: true },
  { id: "focus", title: "Sharpen focus", line: "Steady attention, without strain.", protocolIds: ["focus", "peak-performance"], alpha: false },
  { id: "ideas", title: "Open up ideas", line: "A loose, wandering kind of attention.", protocolIds: ["creative-insight"], alpha: false },
];

export const goalOf = (protocolId: string) => GOALS.find((goal) => goal.protocolIds.includes(protocolId)) ?? GOALS[0];

/** With a back-of-head electrode live, alpha training is where it helps most. */
export const recommendedGoal = (electrodeLive: boolean) => (electrodeLive ? "deep" : null);

export const suggestedStrategy = (protocol: NfProtocol) => protocol.sessionPrompt ?? protocol.description;

/** Plain names for the Muse sensors. */
export const SENSORS: Array<{ channel: string; name: string }> = [
  { channel: "TP9", name: "Left ear" },
  { channel: "AF7", name: "Left forehead" },
  { channel: "AF8", name: "Right forehead" },
  { channel: "TP10", name: "Right ear" },
];

export type NfNote = { at: string; protocol: string; targetPercent: number | null; strategy: string; note: string; sessionId?: string };
export const NOTES_KEY = "neurofeedback-notes";
/** The "What helped?" note of one saved session (the list in NOTES_KEY is still read for "Last time"). */
export const sessionNoteKey = (sessionId: string) => `neurofeedback-note:${sessionId}`;

export function parseNotes(raw: string | null): NfNote[] {
  if (!raw) return [];
  try {
    const value = JSON.parse(raw) as unknown;
    return Array.isArray(value) ? value.filter((item): item is NfNote => !!item && typeof (item as NfNote).at === "string" && typeof (item as NfNote).note === "string") : [];
  } catch { return []; }
}
