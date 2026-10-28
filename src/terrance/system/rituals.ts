/**
 * Rituals — Lightweight scheduled reminders via Telegram.
 * No LLM calls. Static messages at defined times.
 * Sourced from Obsidian: "Active reminders and tasks" + "Alpha Wave Plan"
 */

import { invoke } from "@tauri-apps/api/core";
import { activityBus } from "./activityBus";
import { Backoff } from "./backoff";

// ─── Types ──────────────────────────────────────────────────────────────────────

interface Ritual {
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

interface RitualState {
  lastRunTimestamps: Record<string, number>;
}

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
      state = JSON.parse(raw);
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

/** Check if a ritual should fire right now */
function shouldFire(ritual: Ritual, now: Date): boolean {
  if (!ritual.enabled) return false;

  const lastRun = state.lastRunTimestamps[ritual.id] ?? 0;
  const hoursSinceRun = (now.getTime() - lastRun) / (1000 * 60 * 60);

  // Monthly rituals — check day of month
  if (ritual.dayOfMonth !== undefined) {
    if (now.getDate() !== ritual.dayOfMonth) return false;
    if (hoursSinceRun < 22) return false; // Prevent double-fire
    if (now.getHours() < ritual.hour) return false;
    return true;
  }

  // Weekly rituals — check day of week
  if (ritual.days && ritual.days.length > 0) {
    if (!ritual.days.includes(now.getDay())) return false;
    if (hoursSinceRun < 22) return false;
    if (now.getHours() < ritual.hour) return false;
    return true;
  }

  // Daily rituals — check hour window
  // Fire if current hour matches and we haven't fired in 22+ hours
  if (hoursSinceRun < 22) return false;
  if (now.getHours() < ritual.hour) return false;
  // Don't fire if we're more than 2 hours past the window (stale)
  if (now.getHours() > ritual.hour + 2) return false;

  return true;
}

// Failed sends back off (5/15/60 min, max 3 per day) instead of retrying
// every tick while the ritual stays due. In memory only.
const sendBackoff = new Backoff();

/** Backoff window: one per local calendar day (rituals fire at most daily). */
function dayWindow(now: Date): string {
  return `${now.getFullYear()}-${now.getMonth() + 1}-${now.getDate()}`;
}

/**
 * Called every scheduler tick (5 min). Fires any due rituals.
 */
export async function tickRituals(): Promise<void> {
  await loadState();
  const now = new Date();
  let fired = 0;

  for (const ritual of RITUALS) {
    if (!shouldFire(ritual, now)) continue;
    if (sendBackoff.inBackoff(ritual.id, now.getTime(), dayWindow(now))) continue;

    const taskId = activityBus.register(`Ritual: ${ritual.name}`, "daily_summary");

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
      fired++;
    } catch (e) {
      sendBackoff.recordFailure(ritual.id, now.getTime(), dayWindow(now));
      console.error(`[rituals] Failed to send ${ritual.id}:`, e);
      activityBus.fail(taskId, String(e));
    }
  }

  if (fired > 0) {
    await saveState();
    console.log(`[rituals] Fired ${fired} ritual(s).`);
  }
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
