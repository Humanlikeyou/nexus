import { Fragment, type ReactNode } from "react";
import { DataTag } from "../ui";
import { useAmaraTakeStatus } from "./useAmaraTake";
import { askMore, goToCite, PREVIEW_DEPS, previewMode, sampleTakeView, TAKE_PAGE_LABEL, TAKE_REFRESH_LINE, takeView, type TakeNote, type TakeView } from "./takeView";
import type { TakePage } from "./types";
import { useTakeSlots } from "../takes/TakeExtras";
import { TakeRating } from "../takes/TakeRating";
import { TakeStepQuestion } from "../goals/TakeStepQuestion";
import "./AmaraTakeCard.css";

/** Amara's mark: ember heart, copper core, emerald ring (design-1110f). */
function Ember({ small = false }: { small?: boolean }) {
  return (
    <span className={small ? "take__ember take__ember--sm" : "take__ember"} aria-hidden="true">
      <svg viewBox="0 0 24 24"><path d="M12 21s-7.5-4.6-9.6-9.2C.9 8.4 3 4.5 6.7 4.5c2.1 0 3.7 1.2 5.3 3.1 1.6-1.9 3.2-3.1 5.3-3.1 3.7 0 5.8 3.9 4.3 7.3C19.5 16.4 12 21 12 21z" /></svg>
    </span>
  );
}
export { Ember as TakeEmber };

/** `**74%**` → bold. Generated text has no markup, so only sample notes use it. */
function Body({ text }: { text: string }) {
  const parts = text.split(/\*\*(.+?)\*\*/g);
  return <p className="take__body">{parts.map((p, i) => (i % 2 ? <b key={i}>{p}</b> : <Fragment key={i}>{p}</Fragment>))}</p>;
}

function Head({ lane, children }: { lane?: string; children?: ReactNode }) {
  return (
    <header className="take__head">
      <Ember />
      <h2 className="take__title">Amara&apos;s take</h2>
      {lane && <span className="take__lane">{lane}</span>}
      {children}
    </header>
  );
}

/** design-1111t slots: the memory line under the first sentence, History in the header, today's one thing under the chips. */
export type TakeCardSlots = { memory?: ReactNode; history?: ReactNode; block?: ReactNode; sheet?: ReactNode; refresh?: string };
export type AmaraTakeCardProps = { page: TakePage; view: TakeView; compact?: boolean; onRetry?: () => void; slots?: TakeCardSlots };

/** The card for one state. Pure: screens use <AmaraTake page=… />, tests and previews render states directly. */
export function AmaraTakeCard({ page, view, compact = false, onRetry, slots = {} }: AmaraTakeCardProps) {
  const cls = compact ? "take take--compact" : "take";
  if (view.state === "generating") {
    return (
      <section className={cls} aria-label="Amara's take" aria-busy="true" data-state="generating">
        <Head><span className="take__when">Writing today&apos;s note…</span></Head>
        <div className="take__skel" aria-hidden="true"><i /><i /><i /></div>
        <p className="take__quiet" role="status">Amara is reading today&apos;s data. This takes a few seconds, once a day.</p>
      </section>
    );
  }
  if (view.state === "nodata") {
    return (
      <section className={cls} aria-label="Amara's take" data-state="nodata">
        <Head><span className="take__when">Today</span>{view.sample && <DataTag kind="sample" />}</Head>
        <p className="take__quiet">No new data yet today. {view.waitingFor} Yesterday&apos;s note is in history.</p>
      </section>
    );
  }
  if (view.state === "failed") {
    return (
      <section className={cls} aria-label="Amara's take" data-state="failed">
        <Head><DataTag kind="wire" /></Head>
        <p className="take__quiet">{view.reason}. Nothing here is written by Amara today.{onRetry && <> <button type="button" className="take__link" onClick={onRetry}>Try again</button></>}</p>
      </section>
    );
  }
  if (view.state === "unwired") {
    return (
      <section className={cls} aria-label="Amara's take" data-state="unwired">
        <Head><DataTag kind="wire" /></Head>
        <p className="take__quiet">{compact ? "Amara's daily take isn't written yet. Nothing here is written by Amara today." : "Amara's daily take isn't written yet. It appears once today's data exists and the local note generator answers. Nothing here is written by Amara today."}</p>
      </section>
    );
  }
  const note: TakeNote = view.note;
  return (
    <section className={cls} aria-label="Amara's take" data-state="fresh">
      <Head lane={note.lane}><span className="take__when"><time>{note.time}</time></span>{view.sample && <DataTag kind="sample" />}{slots.history}</Head>
      <Body text={note.text} />
      {slots.memory}
      <ul className="take__chips" aria-label="Numbers Amara used">
        {note.chips.map((c) => {
          // onCite (1110f): a chip with a real source on this page jumps to it; others stay inert.
          const go = c.sourceId ? () => { goToCite(c.sourceId); } : undefined;
          // The link role sits on an inner element: a role on the <li> breaks the list's required children (axe "list").
          const inner = <><span>{c.label}</span>{c.value}</>;
          return <li key={`${c.label}-${c.value}`} className="take__chip" data-lane={c.lane}>
            {go ? <div className="take__chip-link" role="link" tabIndex={0} onClick={go} onKeyDown={(e: React.KeyboardEvent) => { if (e.key === "Enter") go(); }}>{inner}</div> : inner}</li>;
        })}
      </ul>
      {slots.block}
      <div className="take__foot">
        {!compact && <span className="take__src">{note.sourceLine} · {slots.refresh ?? TAKE_REFRESH_LINE[page]}</span>}
        <span className="take__spacer" />
        {slots.sheet}
        {note.takeId && !view.sample && <TakeRating key={note.takeId} page={page} takeId={note.takeId} />}
        <button type="button" className="take__ask" onClick={() => askMore({ from: TAKE_PAGE_LABEL[page], chips: note.chips, question: note.question, sample: view.sample, text: note.text.replace(/\*\*/g, ""), takeId: note.takeId, page })}>Ask more ›</button>
      </div>
    </section>
  );
}

/** Amara's take on a page (design-1110f). Live: useAmaraTake. Preview (sample adapters on): the mockup note, tagged Sample data. */
/** design-1118a: a redesigned page keeps the take under its hero, hidden only while no take has been written yet. */
export const takeWritten = (view: TakeView) => view.state === "fresh" || view.state === "generating";

export function AmaraTake({ page, compact = false, className, hideUntilWritten = false, previewNote }: { page: TakePage; compact?: boolean; className?: string; hideUntilWritten?: boolean; previewNote?: TakeNote }) {
  const preview = previewMode();
  const status = useAmaraTakeStatus(page, preview ? PREVIEW_DEPS : undefined);
  const base = preview ? sampleTakeView(page) : takeView(page, status.value, status.checking);
  const view: TakeView = preview && previewNote && base.state === "fresh" ? { ...base, note: previewNote } : base;
  const slots = useTakeSlots(page, compact);
  if (hideUntilWritten && !takeWritten(view)) return null;
  return <div className={className ? `take-slot ${className}` : "take-slot"}><AmaraTakeCard page={page} view={view} compact={compact} slots={slots} /><TakeStepQuestion /></div>;
}
