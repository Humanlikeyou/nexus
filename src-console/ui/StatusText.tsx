import type { ReactNode } from "react";
import type { Wired } from "../lib/sampleAdapters";
import "./StatusText.css";

/** Status as glyph + word, never colour alone (design-1110d). */
export type StatusKind = "ok" | "fail" | "wait" | "run" | "off" | "risk";
const GLYPH: Record<StatusKind, string> = { ok: "✓", fail: "✕", wait: "◔", run: "▶", off: "–", risk: "▲" };

export function StatusText({ kind, children, className }: { kind: StatusKind; children?: ReactNode; className?: string }) {
  return <span className={className ? `status-text ${className}` : "status-text"} data-s={kind}><span className="status-text__glyph" aria-hidden="true">{GLYPH[kind]}</span>{children}</span>;
}

/** A visible tag for values that are not live: sample values from a preview adapter, or no source yet. */
export function DataTag({ kind }: { kind: "sample" | "wire" | "currency" }) {
  const text = kind === "sample" ? "Sample data" : kind === "wire" ? "Needs wiring" : "Needs wiring · currency conversion";
  return <span className="data-tag" data-kind={kind === "sample" ? "sample" : "wire"}>{text}</span>;
}

/** The tag a Wired value needs, if any. */
export function wiredTag<T>(w: Wired<T>): ReactNode {
  return w.state === "sample" ? <DataTag kind="sample" /> : w.state === "unwired" ? <DataTag kind={w.reason === "currency" ? "currency" : "wire"} /> : null;
}
