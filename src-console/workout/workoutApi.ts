/**
 * Decision 0062 (Phase 9): the Workout engine's commands. Every number (estimate, completion, volume) and
 * every time is the Rust engine's; this file only calls it and formats.
 */
import { invokeBound } from "../bridge";

export type Exercise = {
  id: string; name: string; family: string; level: number; measure: "reps" | "hold"; difficulty: "beginner" | "intermediate" | "advanced";
  description: string; instructions: string[]; muscles: string[]; equipment: string[]; sets: number; reps: number | null; holdS: number | null;
  secsPerRep: number | null; form: string[]; safety: string[]; demoAsset: string | null; custom: boolean;
};
export type TemplateItem = { exerciseId: string; sets: number; reps: number | null; holdS: number | null; loadKg: number | null; restS: number };
export type Template = { id: string; name: string; source: "user" | "amara"; sourceRef: string | null; items: TemplateItem[]; restBetweenS: number; archived: boolean; createdAt: string; updatedAt: string };
export type SetLog = { item: number; set: number; exerciseId: string; outcome: "done" | "skipped"; reps: number | null; holdS: number | null; loadKg: number | null; finishedAt: string; restBeforeS: number | null };
export type SessionState = "planned" | "active" | "paused" | "completed" | "partially_completed" | "abandoned";
export type Session = {
  id: string; templateId: string; templateName: string; source: string; sourceRef: string | null; plan: TemplateItem[]; restBetweenS: number;
  state: SessionState; plannedAt: string; startedAt: string | null; endedAt: string | null; pauses: Array<{ from: string; to: string | null }>;
  sets: SetLog[]; nextItem: number; nextSet: number; engine: string; offerSid: string | null;
};
export type Summary = {
  plannedSets: number; doneSets: number; skippedSets: number; completionPct: number; totalReps: number; totalHoldS: number; loadVolumeKg: number;
  setsByMuscle: Record<string, number>; activeS: number | null; pausedS: number; restS: number; estimatedS: number;
};
export type SessionView = { session: Session; summary: Summary };
export type NoopLink = { startedAt: string; sport: string; durationMin: number | null; strain: number | null };
export type Action =
  | { kind: "start" } | { kind: "pause" } | { kind: "resume" }
  | { kind: "logSet"; reps?: number | null; holdS?: number | null; loadKg?: number | null }
  | { kind: "skip"; scope: "set" | "exercise" }
  | { kind: "end"; abandon?: boolean };

export type WorkoutCalls = {
  library: () => Promise<{ version: string; exercises: Exercise[] }>;
  templates: () => Promise<Template[]>;
  saveTemplate: (t: Template) => Promise<Template>;
  estimate: (items: TemplateItem[], restBetweenS: number) => Promise<number>;
  plan: (templateId: string, offerSid?: string | null) => Promise<SessionView>;
  act: (sessionId: string, action: Action) => Promise<SessionView>;
  history: (limit?: number) => Promise<SessionView[]>;
  /** History with the overlapping NOOP-detected workout, when there is one (a link, never a merge). */
  historyLinked: (limit?: number) => Promise<Array<{ view: SessionView; noop: NoopLink | null }>>;
  fromTake: (takeId: string) => Promise<Template>;
  takeIds: () => Promise<Array<{ takeId: string; name: string; estimatedS: number }>>;
};

export const liveWorkoutCalls: WorkoutCalls = {
  library: () => invokeBound("workout_library"),
  templates: () => invokeBound("workout_templates"),
  saveTemplate: (template) => invokeBound("workout_template_save", { template }),
  estimate: (items, restBetweenS) => invokeBound("workout_estimate", { items, restBetweenS }),
  plan: (templateId, offerSid = null) => invokeBound("workout_session_plan", { templateId, offerSid }),
  act: (sessionId, action) => invokeBound("workout_session_act", { sessionId, action }),
  history: (limit = 50) => invokeBound("workout_history", { limit }),
  historyLinked: (limit = 50) => invokeBound("workout_history_linked", { limit }),
  fromTake: (takeId) => invokeBound("workout_template_from_take", { takeId }),
  takeIds: () => invokeBound("workout_take_ids"),
};

export const OPEN_STATES: SessionState[] = ["planned", "active", "paused"];
export const STATE_LABEL: Record<SessionState, string> = { planned: "Planned", active: "Active", paused: "Paused", completed: "Completed", partially_completed: "Partly completed", abandoned: "Abandoned" };
export const FAMILY_LABEL: Record<string, string> = { push: "Push", pull: "Pull", dip: "Dips", legs: "Legs", core: "Core", skill: "Skills", custom: "Your exercises" };

/** "12 min", "1 h 5 min", "45 s". */
export function duration(s: number | null | undefined): string {
  if (s == null || !Number.isFinite(s)) return "Not recorded";
  if (s < 60) return `${Math.round(s)} s`;
  const m = Math.round(s / 60);
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** What a template item asks for, in words. */
export function prescription(it: TemplateItem): string {
  const work = it.reps != null ? `${it.sets} × ${it.reps}` : `${it.sets} × ${it.holdS} s`;
  return `${work}${it.loadKg ? ` +${it.loadKg} kg` : ""} · rest ${it.restS} s`;
}

/** A new item for an exercise, from its own recommendation. */
export function itemFor(e: Exercise): TemplateItem {
  return { exerciseId: e.id, sets: e.sets, reps: e.measure === "reps" ? e.reps : null, holdS: e.measure === "hold" ? e.holdS : null, loadKg: null, restS: e.measure === "hold" ? 60 : 90 };
}

/** Moves item `i` by `delta` (−1 up, +1 down); a move past either end changes nothing. */
export function move<T>(list: T[], i: number, delta: number): T[] {
  const j = i + delta;
  if (i < 0 || i >= list.length || j < 0 || j >= list.length) return list;
  const out = [...list];
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}
