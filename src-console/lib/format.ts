

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

export function humanize(value: string | null | undefined) {
  const text = String(value ?? "unknown").replace(/[_-]+/g, " ").trim();
  return text ? text.charAt(0).toUpperCase() + text.slice(1).toLowerCase() : "Unknown";
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

export function date(value: string | number | null | undefined) {
  if (!value) return "No record";
  const parsed = new Date(typeof value === "number" && value < 1e12 ? value * 1000 : value);
  if (Number.isNaN(parsed.getTime())) return String(value);
  return parsed.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

export function time(value: string | number | null | undefined) {
  if (!value) return "--";
  const parsed = new Date(typeof value === "number" && value < 1e12 ? value * 1000 : value);
  if (Number.isNaN(parsed.getTime())) return "--";
  return parsed.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
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
  if (message.toLowerCase().includes("permission")) return "Nexus needs Bluetooth permission in System Settings before it can connect.";
  return message.replace(/^Error:\s*/i, "") || "The device request could not be completed.";
}
