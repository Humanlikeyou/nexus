import { useState, type ReactNode } from "react";
import { DataTag } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import type { TakePage } from "../amaraTake/types";
import { takesAdapter, type TakesAdapter } from "./adapters";
import { useTakeLogVersion } from "./takeLog";
import { FollowUpBlock } from "./FollowUp";
import { HistorySheet } from "./HistorySheet";
import { Rich } from "./parts";
import { SuggestionBlock } from "./SuggestionBlock";
import "./takes.css";

export type TakeSlots = { memory: ReactNode; history: ReactNode; block: ReactNode; sheet: ReactNode };

/** The "↺ Oct 7 I suggested…" line: a template over stored rows, never model recall. */
export function MemoryLineView({ text }: { text: string }) {
  return <p className="take__memory"><span className="t-q" aria-hidden="true">↺</span><span><Rich text={text} /></span></p>;
}

/** design-1111t: what the take card adds on top of today's note. The take keeps working when this layer is unwired. */
export function useTakeSlots(page: TakePage, compact: boolean, adapter: TakesAdapter = takesAdapter()): TakeSlots {
  const [open, setOpen] = useState(false);
  useTakeLogVersion(); // re-render when the take log loads
  const today = adapter.today(page);
  const t = valueOf(today);
  const mem = valueOf(adapter.memory(page));
  const count = valueOf(adapter.historyCount(page));
  const sampleTag = today.state === "sample";
  const history = compact ? null : <button type="button" className="take__hist" onClick={() => setOpen(true)}>History{count != null ? ` · ${count} takes` : ""} ›</button>;
  const sheet = compact ? null : <HistorySheet page={page} open={open} onClose={() => setOpen(false)} adapter={adapter} />;
  const memory = !compact && mem ? <MemoryLineView text={mem.text} /> : null;
  let block: ReactNode = null;
  if (!t) block = compact ? null : <div className="t-empty"><b>Today&apos;s one thing</b><span>Suggestions <DataTag kind="wire" /> (the reviewed suggestion list and the answer store). Nothing here is a suggestion from Amara.</span></div>;
  else if (t.kind === "learning") block = compact ? null : <div className="t-empty"><b>No suggestion yet.</b><span>I need {t.need} {t.unit} to know your usual before I suggest anything. {t.have} of {t.need} so far.</span></div>;
  else if (t.kind === "none_today") block = compact ? null : <div className="t-empty"><b>No suggestion today, on purpose.</b><span>{t.why}</span></div>;
  else if (t.kind === "followup") block = compact ? null : <FollowUpBlock o={t.outcome} sample={sampleTag} onAction={(a) => adapter.typeAction(t.outcome.type, a)} />;
  else block = <SuggestionBlock s={t.suggestion} compact={compact} detects={adapter.detects(page)} onAnswer={(st, r) => adapter.answer(page, t.suggestion.id, st, r)} onUndo={() => adapter.undo(page, t.suggestion.id)} />;
  return { memory, history, block, sheet };
}
