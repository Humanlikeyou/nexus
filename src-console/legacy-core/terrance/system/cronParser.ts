/**
 * Natural Language Cron Parser
 *
 * Converts human-readable schedule expressions into millisecond intervals
 * and next-run timestamps. Supports:
 *
 *  - Duration: "every 30m", "every 2h", "every 1d"
 *  - Time of day: "daily at 9am", "daily at 14:30"
 *  - Named: "hourly", "daily", "weekly"
 *  - Standard cron: "0 9 * * *" (5-field cron expression)
 *
 * Inspired by Hermes Agent's schedule parser.
 */

// ─── Types ──────────────────────────────────────────────────────────────────

export interface ParsedSchedule {
  type: "interval" | "daily" | "cron";
  intervalMs?: number;
  hour?: number;
  minute?: number;
  dayOfWeek?: number; // 0=Sun, 1=Mon, ...
  raw: string;
  description: string;
}

// ─── Duration Patterns ──────────────────────────────────────────────────────

const DURATION_RE = /^every\s+(\d+)\s*(m|min|mins|minutes?|h|hr|hrs|hours?|d|days?|w|weeks?|s|sec|secs|seconds?)$/i;
const DAILY_AT_RE = /^daily\s+at\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?$/i;
const NAMED_RE = /^(hourly|daily|weekly|every\s+morning|every\s+evening)$/i;
const CRON_RE = /^(\d+|\*)\s+(\d+|\*)\s+(\d+|\*)\s+(\d+|\*)\s+(\d+|\*)$/;

// Multipliers in milliseconds
const MS = {
  s: 1000,
  m: 60 * 1000,
  h: 60 * 60 * 1000,
  d: 24 * 60 * 60 * 1000,
  w: 7 * 24 * 60 * 60 * 1000,
};

// ─── Parsing ────────────────────────────────────────────────────────────────

export function parseSchedule(input: string): ParsedSchedule | null {
  const trimmed = input.trim();

  // Duration: "every 30m", "every 2 hours"
  const durationMatch = DURATION_RE.exec(trimmed);
  if (durationMatch) {
    const value = parseInt(durationMatch[1], 10);
    const unit = normalizeUnit(durationMatch[2]);
    const multiplier = MS[unit];
    if (!multiplier || value <= 0) return null;
    return {
      type: "interval",
      intervalMs: value * multiplier,
      raw: trimmed,
      description: `every ${value}${unit}`,
    };
  }

  // Daily at time: "daily at 9am", "daily at 14:30"
  const dailyMatch = DAILY_AT_RE.exec(trimmed);
  if (dailyMatch) {
    let hour = parseInt(dailyMatch[1], 10);
    const minute = dailyMatch[2] ? parseInt(dailyMatch[2], 10) : 0;
    const ampm = dailyMatch[3]?.toLowerCase();
    if (ampm === "pm" && hour < 12) hour += 12;
    if (ampm === "am" && hour === 12) hour = 0;
    if (hour < 0 || hour > 23 || minute < 0 || minute > 59) return null;
    return {
      type: "daily",
      hour,
      minute,
      raw: trimmed,
      description: `daily at ${formatTime(hour, minute)}`,
    };
  }

  // Named schedules
  const namedMatch = NAMED_RE.exec(trimmed);
  if (namedMatch) {
    const name = namedMatch[1].toLowerCase();
    switch (name) {
      case "hourly":
        return { type: "interval", intervalMs: MS.h, raw: trimmed, description: "hourly" };
      case "daily":
        return { type: "daily", hour: 7, minute: 0, raw: trimmed, description: "daily at 7:00 AM" };
      case "weekly":
        return { type: "daily", hour: 7, minute: 0, dayOfWeek: 1, raw: trimmed, description: "weekly (Monday 7:00 AM)" };
      case "every morning":
        return { type: "daily", hour: 7, minute: 0, raw: trimmed, description: "every morning (7:00 AM)" };
      case "every evening":
        return { type: "daily", hour: 20, minute: 0, raw: trimmed, description: "every evening (8:00 PM)" };
    }
  }

  // Standard 5-field cron: "0 9 * * *"
  const cronMatch = CRON_RE.exec(trimmed);
  if (cronMatch) {
    const minute = cronMatch[1] === "*" ? 0 : parseInt(cronMatch[1], 10);
    const hour = cronMatch[2] === "*" ? undefined : parseInt(cronMatch[2], 10);
    return {
      type: "cron",
      hour,
      minute,
      raw: trimmed,
      description: `cron: ${trimmed}`,
    };
  }

  return null;
}

// ─── Next Run Calculation ───────────────────────────────────────────────────

/**
 * Calculate the next run time for a parsed schedule.
 * @param schedule The parsed schedule
 * @param lastRun Timestamp of the last run (0 = never run)
 */
export function getNextRunTime(schedule: ParsedSchedule, lastRun: number): number {
  const now = Date.now();

  if (schedule.type === "interval" && schedule.intervalMs) {
    if (lastRun === 0) return now; // Run immediately on first schedule
    return lastRun + schedule.intervalMs;
  }

  if (schedule.type === "daily" || schedule.type === "cron") {
    const hour = schedule.hour ?? 0;
    const minute = schedule.minute ?? 0;

    const next = new Date();
    next.setHours(hour, minute, 0, 0);

    // If the time already passed today, schedule for tomorrow
    if (next.getTime() <= now) {
      next.setDate(next.getDate() + 1);
    }

    // Weekly: advance to the correct day
    if (schedule.dayOfWeek !== undefined) {
      while (next.getDay() !== schedule.dayOfWeek) {
        next.setDate(next.getDate() + 1);
      }
    }

    return next.getTime();
  }

  return now;
}

/**
 * Check if a scheduled job should run now.
 */
export function shouldRunNow(schedule: ParsedSchedule, lastRun: number): boolean {
  const nextRun = getNextRunTime(schedule, lastRun);
  return Date.now() >= nextRun;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function normalizeUnit(unit: string): "s" | "m" | "h" | "d" | "w" {
  const u = unit.toLowerCase();
  if (u.startsWith("s")) return "s";
  if (u.startsWith("m")) return "m";
  if (u.startsWith("h")) return "h";
  if (u.startsWith("d")) return "d";
  if (u.startsWith("w")) return "w";
  return "m";
}

function formatTime(hour: number, minute: number): string {
  const ampm = hour >= 12 ? "PM" : "AM";
  const h = hour % 12 || 12;
  const m = minute.toString().padStart(2, "0");
  return `${h}:${m} ${ampm}`;
}
