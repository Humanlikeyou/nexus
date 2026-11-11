import type { ReactNode } from "react";
import { Card, DataTag, StatusText, wiredTag } from "../ui";
import type { Wired } from "../lib/sampleAdapters";
import type { ServiceState } from "./adapters";

/** One quiet row: title, optional subtitle, trailing accessory (mockup `m-row`). */
export function Row({ title, sub, children }: { title: ReactNode; sub?: ReactNode; children?: ReactNode }) {
  return <div className="pk-row"><div className="pk-row__main"><span>{title}</span>{sub && <small>{sub}</small>}</div>{children}</div>;
}

/** A spinner + word, for "Restarting", "Downloading", "Connecting". */
export function Spin({ children }: { children: ReactNode }) {
  return <span className="status-text" data-s="run"><span className="pk-spin" aria-hidden="true" />{children}</span>;
}

/** Card with the Wired tag in its header accessory. */
export function SaCard({ title, w, tag, accessory, children, className }: { title: ReactNode; w?: Wired<unknown>; tag?: "sample" | "wire"; accessory?: ReactNode; children: ReactNode; className?: string }) {
  const t = w ? wiredTag(w) : tag ? <DataTag kind={tag} /> : null;
  return <Card title={title} className={className} accessory={(t || accessory) ? <span className="pk-acc">{t}{accessory}</span> : undefined}>{children}</Card>;
}

export function Bar({ pct, label }: { pct: number; label?: string }) {
  const v = Math.max(0, Math.min(100, Math.round(pct)));
  return <div className="pk-bar" role="progressbar" aria-label={label ?? "Progress"} aria-valuemin={0} aria-valuemax={100} aria-valuenow={v}><i style={{ width: `${v}%` }} /></div>;
}

/** Set / Not set: the only thing a key field ever reports. */
export function KeyState({ set }: { set: boolean }) {
  return set ? <StatusText kind="ok">Set</StatusText> : <StatusText kind="off">Not set</StatusText>;
}

const SERVICE_WORD: Record<ServiceState, string> = { healthy: "Healthy", degraded: "Slow", restarting: "Restarting", stopped: "Stopped", waiting: "Waiting", failed: "Not running" };
export function ServiceStatus({ state }: { state: ServiceState }) {
  if (state === "restarting") return <Spin>{SERVICE_WORD[state]}</Spin>;
  const kind = state === "healthy" ? "ok" : state === "degraded" ? "wait" : state === "failed" ? "fail" : "off";
  return <StatusText kind={kind}>{SERVICE_WORD[state]}</StatusText>;
}
export const serviceWord = (s: ServiceState) => SERVICE_WORD[s];

/** `**bold**` in sample copy. */
export function Rich({ text }: { text: string }) {
  return <>{text.split(/\*\*(.+?)\*\*/g).map((p, i) => (i % 2 ? <b key={i}>{p}</b> : <span key={i}>{p}</span>))}</>;
}

export const WIRE_NOTE = "Needs wiring: this fills in once Claude builds the backend (docs/design/HANDOFF-1111s.md). Nothing here is made up.";
export function WireNote({ children }: { children?: ReactNode }) {
  return <p className="pk-note">{children ?? WIRE_NOTE} <DataTag kind="wire" /></p>;
}
