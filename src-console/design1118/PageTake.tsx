import { AmaraTake, AmaraTakeCard } from "../amaraTake/AmaraTakeCard";
import { previewMode, type TakeNote } from "../amaraTake/takeView";
import type { TakePage } from "../amaraTake/types";

/**
 * design-1118a §10 "Amara's take": every redesigned page keeps a take card in a prominent spot,
 * directly under the hero. It is hidden only while no take has been written yet, never removed.
 *
 * Pages with a take generator (life, neuro, business) use the live take. Agents and Smart home have
 * no generator yet (Needs wiring, Claude): in the app the card stays hidden until one is written;
 * previews show the round-5 sample note, labelled Sample data.
 */
export type TakeSurface = "goals" | "neuro" | "hemisync" | "ventures" | "finances" | "content" | "agents" | "smarthome";

const note = (text: string, sourceLine: string, question: string): TakeNote => ({ text, chips: [], time: "Today", sourceLine, question });

/** Round-5 final-still notes (preview / tests only, always tagged Sample data). */
export const PREVIEW_NOTES: Record<TakeSurface, TakeNote> = {
  goals: note("The next move on Taiyo Skin is the manufacturer contract. Link a source for your habits and the week will start counting.", "From Life", "What's the smallest next step?"),
  neuro: note("Last session was five minutes on Sunday. Recovery that day and recovery now are kept apart. Find Muse when you want the next one.", "From Neuro", "When should I do the next session?"),
  hemisync: note("You are in Threshold. Problem Solving is the next track, and it plays only while the Muse records.", "From Hemi-Sync", "Which track next?"),
  ventures: note("Trading is within limits. The ideas below are proposals only. Approving one records your choice; no order is sent.", "From Enterprise", "Why these two ideas?"),
  finances: note("No revenue has reported in yet. Fund rows stay folded until there is a balance.", "From Enterprise", "What will show up first?"),
  content: note("One idea is waiting at Idea. Draft it with me when you're ready; I never post for you.", "From Enterprise", "Draft the first one?"),
  agents: note("Amara is online. Health checks ran this morning. Nothing has been delivered yet.", "From Agents", "What ran today?"),
  smarthome: note("Household controls are grouped by type. Backup sensors stay out of those groups.", "From Smart home", "What's on right now?"),
};

const GENERATOR: Partial<Record<TakeSurface, TakePage>> = { goals: "life", neuro: "neuro", hemisync: "neuro", ventures: "business", finances: "business", content: "business" };

export function PageTake({ surface, className }: { surface: TakeSurface; className?: string }) {
  const page = GENERATOR[surface];
  const cls = ["d18-take", className].filter(Boolean).join(" ");
  if (page) return <AmaraTake page={page} className={cls} hideUntilWritten previewNote={PREVIEW_NOTES[surface]} />;
  if (!previewMode()) return null; // no generator for this page yet: nothing written, so the card stays hidden
  return <div className={`take-slot ${cls}`}><AmaraTakeCard page="home" view={{ state: "fresh", note: PREVIEW_NOTES[surface], sample: true }} /></div>;
}
