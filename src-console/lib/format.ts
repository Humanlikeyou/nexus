

export type Tone = "live" | "watch" | "fault" | "quiet";

export function tone(value: string | boolean | null | undefined): Tone {
  if (value === true) return "live";
  if (!value) return "fault";
  const text = String(value).toLowerCase();
  if (["healthy", "online", "active", "connected", "configured", "complete", "streaming", "enabled", "watching"].some((item) => text.includes(item))) return "live";
  if (["waiting", "degraded", "watch", "testing", "paused", "pending", "unknown", "proxy"].some((item) => text.includes(item))) return "watch";
  if (["offline", "error", "failed", "missing", "disconnected", "killed", "down"].some((item) => text.includes(item))) return "fault";
  return "quiet";
}

/** Whole-word tokens kept upper case after sentence-casing ("qa" → "QA", "api_service" → "API service"). */
const ACRONYMS = new Set(["qa", "ui", "api", "ai", "llm", "seo"]);

export function humanize(value: string | null | undefined) {
  const text = String(value ?? "unknown").replace(/[_-]+/g, " ").trim();
  if (!text) return "Unknown";
  const sentence = text.charAt(0).toUpperCase() + text.slice(1).toLowerCase();
  return sentence
    .replace(/\b[a-z]+\b/gi, (word) => (ACRONYMS.has(word.toLowerCase()) ? word.toUpperCase() : word))
    .replace(/\be commerce\b/i, (m) => (m[0] === "E" ? "E-commerce" : "e-commerce"));
}

/** Data titles start with a capital ("open app" → "Open app"); the rest is left as written (P0.5). */
export function sentenceStart(value: string) {
  const text = value.trimStart();
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : value;
}

export function plainText(value: string) {
  return value
    .replace(/^\s*[-*]\s+/, "")
    .replace(/^\s*\[[ x~>-]\]\s*/i, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .trim();
}

export function number(value: number | null | undefined, digits = 0) {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(digits) : "--";
}

export function money(value: number) {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }).format(value || 0);
}

type Instant = string | number | Date | null | undefined;

/** Seconds (< 1e12) or milliseconds, ISO strings and Dates. Null when missing or unparsable. */
function instant(value: Instant): Date | null {
  if (!value) return null;
  const parsed = value instanceof Date ? value : new Date(typeof value === "number" && value < 1e12 ? value * 1000 : value);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

const DATE = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const TIME = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

/** "Oct 28": a calendar date, no time. */
export function date(value: Instant) {
  const parsed = instant(value);
  return parsed ? DATE.format(parsed) : value ? String(value) : "No record";
}

/** "9:00 PM": numeric hour (no leading zero), two-digit minute. */
export function time(value: Instant) {
  const parsed = instant(value);
  return parsed ? TIME.format(parsed) : "--";
}

/** "Oct 28, 1:38 PM": the one timestamp format (never "Oct 28 at 01:38 PM"). */
export function dateTime(value: Instant) {
  const parsed = instant(value);
  return parsed ? `${DATE.format(parsed)}, ${TIME.format(parsed)}` : value ? String(value) : "No record";
}

export function formatMinutes(value: number | null | undefined) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) return "--";
  const rounded = Math.round(value);
  if (rounded < 60) return `${rounded}m`;
  return `${Math.floor(rounded / 60)}h ${rounded % 60}m`;
}

export function shortDuration(ms: number) {
  if (ms < 1000) return `${ms || 0}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms / 60_000)}m`;
}

export function readableError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  if (message.toLowerCase().includes("bluetooth adapter")) return "Bluetooth is unavailable. Check that Bluetooth is on, then try again.";
  if (message.toLowerCase().includes("not found") || message.toLowerCase().includes("not in discovered")) return "That Muse is no longer available. Put it back in pairing mode and scan again.";
  if (message.toLowerCase().includes("permission")) return "Amara OS needs Bluetooth permission in System Settings before it can connect.";
  return message.replace(/^Error:\s*/i, "") || "The device request could not be completed.";
}
