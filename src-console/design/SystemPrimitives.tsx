import type { CSSProperties, ReactNode } from "react";

export function CommandSurface({ mode, children }: { mode: string; children: ReactNode }) {
  return <main className={`command-surface command-surface--${mode}`}>{children}</main>;
}

export function IntelligenceOverlay({ children }: { children: ReactNode }) {
  return <div className="intelligence-overlay">{children}</div>;
}

export function SignalStream({ active = true, className = "" }: { active?: boolean; className?: string }) {
  return <svg className={`signal-stream ${active ? "is-live" : ""} ${className}`} viewBox="0 0 800 46" preserveAspectRatio="none" aria-hidden="true"><path d="M0 24H105l12-4 8 13 12-27 12 36 13-18h91l9-5 8 10 12-9 11 5h102l11-18 12 34 15-17h88l12-4 9 9 10-7 12 4h93l14-21 12 36 16-18h97" /></svg>;
}

export function BiometricReadout({ value, max = 100, label, unit, digits = 0, tint = "cyan" }: { value: number | null | undefined; max?: number; label: string; unit?: string; digits?: number; tint?: "cyan" | "lime" | "amber" | "coral" }) {
  const normalized = Math.max(0, Math.min(1, (value ?? 0) / max));
  const display = typeof value === "number" && Number.isFinite(value) ? value.toFixed(digits) : "--";
  return <div className={`biometric-readout biometric-readout--${tint}`}>
    <svg viewBox="0 0 100 58" aria-hidden="true"><path className="biometric-readout__base" d="M10 52 A42 42 0 0 1 90 52" /><path className="biometric-readout__value" d="M10 52 A42 42 0 0 1 90 52" style={{ strokeDashoffset: 226 * (1 - normalized) } as CSSProperties} /></svg>
    <strong>{display}<small>{unit}</small></strong><span>{label}</span>
  </div>;
}
