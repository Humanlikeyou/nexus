import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatRelativeTime(timestamp: string | number): string {
  const date = new Date(timestamp);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSecs = Math.floor(diffMs / 1000);

  if (diffSecs < 60) return `${diffSecs}s ago`;
  if (diffSecs < 3600) return `${Math.floor(diffSecs / 60)}m ago`;
  if (diffSecs < 86400) return `${Math.floor(diffSecs / 3600)}h ago`;
  return `${Math.floor(diffSecs / 86400)}d ago`;
}

export function formatCountdown(targetDate: string): string {
  const target = new Date(targetDate);
  const now = new Date();
  const diffMs = target.getTime() - now.getTime();

  if (diffMs <= 0) return "now";
  const hours = Math.floor(diffMs / 3600000);
  const mins = Math.floor((diffMs % 3600000) / 60000);

  if (hours > 0) return `in ${hours}h ${mins}m`;
  return `in ${mins}m`;
}

export function computeFocusScore(beta: number, theta: number): number {
  if (theta === 0) return 0;
  const ratio = beta / theta;
  return Math.min(100, Math.max(0, Math.round(ratio * 25)));
}

export function computeHRV(rrIntervals: number[]): {
  rmssd: number;
  sdnn: number;
} {
  if (rrIntervals.length < 2)
    return { rmssd: 0, sdnn: 0 };

  // RMSSD
  let sumSquaredDiffs = 0;
  for (let i = 1; i < rrIntervals.length; i++) {
    const diff = rrIntervals[i] - rrIntervals[i - 1];
    sumSquaredDiffs += diff * diff;
  }
  const rmssd = Math.sqrt(sumSquaredDiffs / (rrIntervals.length - 1));

  // SDNN
  const mean = rrIntervals.reduce((a, b) => a + b, 0) / rrIntervals.length;
  const sumSquaredDeviations = rrIntervals.reduce(
    (sum, rr) => sum + Math.pow(rr - mean, 2),
    0,
  );
  const sdnn = Math.sqrt(sumSquaredDeviations / rrIntervals.length);

  return { rmssd: Math.round(rmssd * 10) / 10, sdnn: Math.round(sdnn * 10) / 10 };
}
