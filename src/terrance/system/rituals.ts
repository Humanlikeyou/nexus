/**
 * Rituals — Lightweight scheduled reminders via Telegram.
 * No LLM calls. Static messages at defined times.
 * Sourced from Obsidian: "Active reminders and tasks" + "Alpha Wave Plan"
 */

import { invoke } from "@tauri-apps/api/core";
import { activityBus } from "./activityBus";
import { Backoff } from "./backoff";
import { isOutboundSuppressed, jobFailureReason } from "./continuity";

// ─── Types ──────────────────────────────────────────────────────────────────────

export interface Ritual {
  id: string;
  name: string;
  message: string;
  /** Hour (0-23) in local time */
  hour: number;
  /** Minute (0-59) */
  minute: number;
  /** Which days this runs. Omit for daily. 0=Sun, 1=Mon, ... 6=Sat */
  days?: number[];
  /** Day of month (1-31) for monthly rituals */
  dayOfMonth?: number;
  category: "morning" | "midday" | "evening" | "weekly" | "monthly";
  enabled: boolean;
}

export interface RitualState {
  lastRunTimestamps: Record<string, number>;
  /** Ritual id -> local day (dayWindow) it was skipped because its window had passed. */
  skippedDays?: Record<string, string>;
}

/** A ritual only fires within this long after its scheduled time; later it is skipped for the day. */
export const RITUAL_WINDOW_MS = 30 * 60_000;

// ─── Ritual Definitions ─────────────────────────────────────────────────────────

const RITUALS: Ritual[] = [
  // ── MORNING ──
  {
    id: "presence-drill",
    name: "Presence Drill",
    message:
      "Presence Drill.\n\n" +
      "Silence — 30 seconds, no input.\n" +
      "Gaze — soft focus, peripheral awareness.\n" +
      "Smile — subtle, internal.\n" +
      "Energy — feel the space around you.\n\n" +
      "3 minutes. Start now.",
    hour: 8,
    minute: 0,
    category: "morning",
    enabled: true,
  },
  {
    id: "command-phrase",
    name: "Command Phrase + Intent",
    message:
      "Set your intent.\n\n" +
      "One directive for today. Say it internally.\n" +
      "Visualize the day executing from that frame.\n\n" +
      "What's the one thing that matters most today?",
    hour: 8,
    minute: 5,
    category: "morning",
    enabled: true,
  },
  {
    id: "alpha-breathing",
    name: "Alpha Breathing",
    message:
      "Alpha Baseline — 3 minutes.\n\n" +
      "Inhale 4s through nose.\n" +
      "Exhale 6-8s through nose.\n" +
      "Soften your gaze. Widen peripheral awareness.\n\n" +
      "Then: 3 min sunlight + stillness. No phone.",
    hour: 8,
    minute: 10,
    category: "morning",
    enabled: true,
  },

  // ── MIDDAY ──
  {
    id: "focus-reset",
    name: "Focus Reset",
    message:
      "Focus reset.\n\n" +
      "What matters right now? Are you drifting?\n" +
      "Peripheral vision reset: soften gaze, widen visual field, 3 slow breaths.\n\n" +
      "Shifts beta overload → alpha. 60 seconds.",
    hour: 12,
    minute: 30,
    category: "midday",
    enabled: true,
  },
  {
    id: "social-expansion",
    name: "Social Expansion",
    message:
      "Social expansion trigger.\n\n" +
      "Talk to one new person today.\n" +
      "Or deepen one existing interaction.\n\n" +
      "Not performance. Connection.",
    hour: 15,
    minute: 0,
    category: "midday",
    enabled: true,
  },

  // ── EVENING ──
  {
    id: "training-reminder",
    name: "Training / Physical Discipline",
    message:
      "Training window open.\n\n" +
      "Run, aikido, mobility, or strength.\n" +
      "Move with intention. Not obligation.",
    hour: 18,
    minute: 0,
    category: "evening",
    enabled: true,
  },
  {
    id: "build-session",
    name: "Build Session",
    message:
      "Build session.\n\n" +
      "TaiyoSkin, filmmaking, Nexus, or creative output.\n" +
      "Pick one. Deep work. No multitasking.",
    hour: 19,
    minute: 30,
    category: "evening",
    enabled: true,
  },
  {
    id: "evening-decompression",
    name: "Evening Decompression",
    message:
      "Night protocol.\n\n" +
      "Slow breathing — 3 min.\n" +
      "Eyes half closed, body scan — 5 min.\n" +
      "Dim lights. Reduce screens in next 60 min.\n\n" +
      "Visualize calm authority. Steady forward motion.",
    hour: 21,
    minute: 30,
    category: "evening",
    enabled: true,
  },

  // ── WEEKLY ──
  {
    id: "weekly-target-lock",
    name: "Weekly Target Lock",
    message:
      "Monday target lock.\n\n" +
      "Define 3 outcomes for this week.\n" +
      "Not tasks — outcomes. What's different by Friday?\n\n" +
      "Reply with your 3 targets.",
    hour: 8,
    minute: 30,
    days: [1], // Monday
    category: "weekly",
    enabled: true,
  },
  {
    id: "midweek-adjustment",
    name: "Midweek Adjustment",
    message:
      "Wednesday check.\n\n" +
      "What needs correction this week?\n" +
      "What's on track? What's slipping?\n\n" +
      "Adjust now, not Friday.",
    hour: 20,
    minute: 30,
    days: [3], // Wednesday
    category: "weekly",
    enabled: true,
  },
  {
    id: "strategic-reflection",
    name: "Strategic Reflection",
    message:
      "Sunday reflection.\n\n" +
      "What worked this week?\n" +
      "What didn't?\n" +
      "What did you learn?\n\n" +
      "One insight to carry forward.",
    hour: 19,
    minute: 0,
    days: [0], // Sunday
    category: "weekly",
    enabled: true,
  },

  // ── MONTHLY ──
  {
    id: "financial-review",
    name: "Financial Review",
    message:
      "1st of the month — financial review.\n\n" +
      "Check: runway, burn rate, income streams, stability.\n" +
      "Update the numbers. Face the reality.\n\n" +
      "Financial clarity = emotional clarity.",
    hour: 9,
    minute: 0,
    dayOfMonth: 1,
    category: "monthly",
    enabled: true,
  },
  {
    id: "trajectory-check",
    name: "Trajectory Check",
    message:
      "Mid-month trajectory check.\n\n" +
      "Are your actions aligned with your long-term identity?\n" +
      "Mission pillars: TaiyoSkin → Creative → Personal mastery → Legacy.\n\n" +
      "Where are you drifting? Where are you compounding?",
    hour: 9,
    minute: 0,
    dayOfMonth: 15,
    category: "monthly",
    enabled: true,
  },
];

// Micro-habits — injected into midday focus reset occasionally
const MICRO_HABITS = [
  "Soften your eyes when speaking.",
  "Unclench jaw. Relax tongue.",
  "Breathe through your nose.",
  "Pause before responding.",
];

// ─── State ──────────────────────────────────────────────────────────────────────

let state: RitualState = { lastRunTimestamps: {} };
const STATE_KEY = "ritual_timestamps";

async function loadState(): Promise<void> {
  try {
    const raw = await invoke<string | null>("load_kv", { key: STATE_KEY });
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<RitualState>;
      state = { ...parsed, lastRunTimestamps: parsed.lastRunTimestamps ?? {} };
    }
  } catch {
    // First run — no state yet, use defaults
  }
}

async function saveState(): Promise<void> {
  try {
    await invoke<void>("save_kv", { key: STATE_KEY, value: JSON.stringify(state) });
  } catch (e) {
    console.error("[rituals] Failed to save state:", e);
  }
}

// ─── Tick Logic ─────────────────────────────────────────────────────────────────

/** Whether the ritual is scheduled on `now`'s local day (daily, weekly day, or monthly date). */
function scheduledToday(ritual: Ritual, now: Date): boolean {
  if (ritual.dayOfMonth !== undefined) return now.getDate() === ritual.dayOfMonth;
  if (ritual.days && ritual.days.length > 0) return ritual.days.includes(now.getDay());
  return true;
}

export interface RitualPlan {
  /** At most one ritual to send this tick: the earliest one still inside its 30-min window. */
  fire: Ritual | null;
  /** Rituals whose window passed today without a send: mark skipped, send nothing. */
  skip: Ritual[];
}

/**
 * Decide what this tick does. Never a backlog: a ritual fires only within 30 min of its
 * scheduled time today; past that it is skipped for the day (live check 2026-10-29: a 10:39
 * launch fired all three 08:00-08:10 rituals at once). One send per tick at most.
 */
export function planRituals(rituals: Ritual[], st: RitualState, now: Date, isBackedOff: (id: string) => boolean = () => false): RitualPlan {
  const today = dayWindow(now);
  const plan: RitualPlan = { fire: null, skip: [] };
  const due = rituals
    .filter((r) => r.enabled && scheduledToday(r, now))
    .map((r) => ({ r, at: new Date(now.getFullYear(), now.getMonth(), now.getDate(), r.hour, r.minute).getTime() }))
    .sort((a, b) => a.at - b.at);
  for (const { r, at } of due) {
    const last = st.lastRunTimestamps[r.id];
    if (last != null && dayWindow(new Date(last)) === today) continue;
    if (st.skippedDays?.[r.id] === today) continue;
    if (now.getTime() < at) continue;
    if (now.getTime() - at > RITUAL_WINDOW_MS) plan.skip.push(r);
    else if (!plan.fire && !isBackedOff(r.id)) plan.fire = r;
  }
  return plan;
}

// Failed sends back off (5/15/60 min, max 3 per day) instead of retrying
// every tick while the ritual stays due. In memory only.
const sendBackoff = new Backoff();

/** Backoff window: one per local calendar day (rituals fire at most daily). */
function dayWindow(now: Date): string {
  return `${now.getFullYear()}-${now.getMonth() + 1}-${now.getDate()}`;
}

/**
 * Called every scheduler tick (5 min). Sends at most one ritual that is inside its window and
 * records missed ones as "Skipped (missed window)" without sending anything.
 */
export async function tickRituals(): Promise<void> {
  await loadState();
  const now = new Date();
  const today = dayWindow(now);
  const plan = planRituals(RITUALS, state, now, (id) => sendBackoff.inBackoff(id, now.getTime(), today));
  let changed = false;

  for (const ritual of plan.skip) {
    state.skippedDays = { ...(state.skippedDays ?? {}), [ritual.id]: today };
    changed = true;
    // Retried inside its window and failed again: both failures are already in Activity, so the
    // window closing is not a second "missed" entry (one follow-up per failure, never both).
    if (sendBackoff.failureCount(ritual.id, today) >= 2) continue;
    const taskId = activityBus.register(`Ritual: ${ritual.name}`, "ritual");
    activityBus.skip(taskId, "Skipped (missed window)");
  }

  const ritual = plan.fire;
  if (ritual) {
    const taskId = activityBus.register(`Ritual: ${ritual.name}`, "ritual");

    let message = ritual.message;

    // Occasionally append a micro-habit to the focus reset
    if (ritual.id === "focus-reset" && Math.random() < 0.5) {
      const habit = MICRO_HABITS[Math.floor(Math.random() * MICRO_HABITS.length)];
      message += `\n\nMicro-habit: ${habit}`;
    }

    try {
      await invoke<void>("send_telegram_message", { text: message });
      state.lastRunTimestamps[ritual.id] = now.getTime();
      sendBackoff.recordSuccess(ritual.id);
      activityBus.complete(taskId, `Sent: ${ritual.name}`);
      changed = true;
      console.log(`[rituals] Fired ${ritual.id}.`);
    } catch (e) {
      if (isOutboundSuppressed(e)) {
        // Lead decision 2026-10-29: a kill-switch suppression is neutral ("Suppressed"), not a
        // failure: no backoff, and done for today so it isn't re-suppressed every tick.
        state.skippedDays = { ...(state.skippedDays ?? {}), [ritual.id]: today };
        activityBus.suppress(taskId, "Outbound sends disabled (NEXUS_OUTBOUND_DISABLED)");
        changed = true;
      } else {
        sendBackoff.recordFailure(ritual.id, now.getTime(), today);
        console.error(`[rituals] Failed to send ${ritual.id}:`, e);
        activityBus.fail(taskId, jobFailureReason(e, "delivery failed"));
      }
    }
  }

  if (changed) await saveState();
}

/**
 * Get all ritual definitions (for UI display).
 */
export function listRituals(): Ritual[] {
  return RITUALS;
}

/**
 * Toggle a ritual on/off by ID.
 */
export function toggleRitual(id: string): boolean {
  const ritual = RITUALS.find((r) => r.id === id);
  if (!ritual) return false;
  ritual.enabled = !ritual.enabled;
  return ritual.enabled;
}
