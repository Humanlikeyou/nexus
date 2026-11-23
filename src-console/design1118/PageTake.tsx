import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import type { TakeNote } from "../amaraTake/takeView";
import type { TakePage } from "../amaraTake/types";

/**
 * design-1118a §10 "Amara's take": every redesigned page keeps a take card in a prominent spot,
 * directly under the hero. It is hidden only while no take has been written yet, never removed.
 *
 * Every page has a take generator (life, neuro, business, agents, smarthome) and uses the live take.
 * Until a take is written the card stays hidden; previews show the round-5 sample note, labelled Sample data.
 */
export type TakeSurface = "goals" | "neuro" | "hemisync" | "ventures" | "finances" | "content" | "agents" | "smarthome";

const note = (text: string, sourceLine: string, question: string): TakeNote => ({ text, chips: [], time: "Today", sourceLine, question });

/** Round-5 final-still notes (preview / tests only, always tagged Sample data). Live takes come from the take generator, never from these lines, so a sample must not state a journey position as fact. */
export const PREVIEW_NOTES: Record<TakeSurface, TakeNote> = {
  goals: note("The next move on Taiyo Skin is the manufacturer contract. Link a source for your habits and the week will start counting.", "From Life", "What's the smallest next step?"),
  neuro: note("Last session was five minutes on Sunday. Recovery that day and recovery now are kept apart. Find Muse when you want the next one.", "From Neuro", "When should I do the next session?"),
  hemisync: note("Sample note: once you have recorded a Hemi-Sync session, your next track is named here. A track plays only while the Muse records.", "From Hemi-Sync", "Which track next?"),
  ventures: note("Trading is within limits. The ideas below are proposals only. Approving one records your choice; no order is sent.", "From Enterprise", "Why these two ideas?"),
  finances: note("No revenue has reported in yet. Fund rows stay folded until there is a balance.", "From Enterprise", "What will show up first?"),
  content: note("One idea is waiting at Idea. Draft it with me when you're ready; I never post for you.", "From Enterprise", "Draft the first one?"),
  agents: note("Amara is online. Health checks ran this morning. Nothing has been delivered yet.", "From Agents", "What ran today?"),
  smarthome: note("Household controls are grouped by type. Backup sensors stay out of those groups.", "From Smart home", "What's on right now?"),
};

const GENERATOR: Record<TakeSurface, TakePage> = { goals: "life", neuro: "neuro", hemisync: "neuro", ventures: "business", finances: "business", content: "business", agents: "agents", smarthome: "smarthome" };

export function PageTake({ surface, className }: { surface: TakeSurface; className?: string }) {
  const cls = ["d18-take", className].filter(Boolean).join(" ");
  return <AmaraTake page={GENERATOR[surface]} className={cls} hideUntilWritten previewNote={PREVIEW_NOTES[surface]} />;
}
