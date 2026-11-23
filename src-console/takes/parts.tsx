import { Fragment, type ReactNode } from "react";
import { LEVEL_BARS, LEVEL_WORD, VERDICT_SHORT, VERDICT_WORD, type AnswerState, type EvidenceLevel, type Source, type Verdict } from "./adapters";

/** `**bold**` in sample and templated copy (numbers come from the source rows, never the model). */
export function Rich({ text }: { text: string }) {
  return <>{text.split(/\*\*(.+?)\*\*/g).map((p, i) => (i % 2 ? <b key={i}>{p}</b> : <Fragment key={i}>{p}</Fragment>))}</>;
}

/** Evidence level: four squares and the word. "Your data only" has no outside claim, so no squares are lit. */
export function Level({ level }: { level: EvidenceLevel }) {
  const n = LEVEL_BARS[level];
  return (
    <span className="t-lvl" data-l={level} aria-label={`Evidence: ${LEVEL_WORD[level]}`}>
      {level !== "own" && <span aria-hidden="true" className="t-lvl__bars">{[1, 2, 3, 4].map((i) => <i key={i} data-on={i <= n || undefined} />)}</span>}
      {LEVEL_WORD[level]}
    </span>
  );
}

const VERDICT_GLYPH: Record<Verdict, string> = { too_early: "◔", none: "=", better: "▲", worse: "▼", unclear: "?" };
/** Verdict words are fixed and come from code (HANDOFF-1111t §5), never from the model. */
export function VerdictChip({ v, short = false }: { v: Verdict; short?: boolean }) {
  return <span className="t-verdict" data-v={v}><span aria-hidden="true">{VERDICT_GLYPH[v]}</span>{short ? VERDICT_SHORT[v] : VERDICT_WORD[v]}</span>;
}

/** A catalog source. Until a person checks it, it says so (Taisun: placeholder citations stay marked). */
export function SourceLine({ s }: { s: Source }) {
  return <li className="t-src">{s.cite}{!s.verified && <span className="t-placeholder">Placeholder citation · not checked yet</span>}</li>;
}

const DOT_WORD: Record<AnswerState | "detected", string> = { waiting: "waiting", done: "done by you", detected: "done, detected", skipped: "skipped", not_today: "not today", unknown: "unknown" };
const DOT_S: Record<AnswerState | "detected", string> = { waiting: "w", done: "d", detected: "a", skipped: "s", not_today: "n", unknown: "u" };
/** One square per day; a gap is a day nothing was suggested. Labelled with the words, never colour alone. */
export function AnswerDots({ days, label }: { days: Array<AnswerState | "detected" | null>; label: string }) {
  return (
    <span className="t-dots" role="img" aria-label={`${label}: ${days.map((d) => (d ? DOT_WORD[d] : "no suggestion")).join(", ")}`}>
      {days.map((d, i) => <i key={i} data-s={d ? DOT_S[d] : "x"} />)}
    </span>
  );
}

export function DotsLegend() {
  return (
    <div className="t-legend" aria-hidden="true">
      <span><i data-s="d" />done by you</span><span><i data-s="a" />detected</span><span><i data-s="s" />skipped</span><span><i data-s="n" />not today</span><span><i data-s="u" />unknown</span>
    </div>
  );
}

export function Nums({ rows }: { rows: Array<{ label: ReactNode; value: ReactNode; note: ReactNode }> }) {
  return <dl className="t-num">{rows.map((r, i) => <Fragment key={i}><dt>{r.label}</dt><dd className="t-num__v">{r.value}</dd><dd className="t-num__n">{r.note}</dd></Fragment>)}</dl>;
}
