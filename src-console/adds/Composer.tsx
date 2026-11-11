import { Mic } from "lucide-react";
import { useCallback, useEffect, useId, useReducer, useRef, useState, type ReactNode } from "react";
import type { Route } from "../app/routes";
import { valueOf } from "../lib/sampleAdapters";
import { Button, Card, DataTag, Dialog } from "../ui";
import { IDLE, UNDO_MS, busy, flow, type AddAdapter, type AddPreview, type Saved } from "./flow";
import "../takes/takes.css";
import "./v1111.css";

/** Disabled with its reason, never removed (HANDOFF-1111v §3). */
export function OffButton({ children, reason, variant = "secondary", label }: { children: ReactNode; reason: string; variant?: "primary" | "secondary" | "plain"; label?: string }) {
  const id = useId();
  return <span className="v-off"><Button size="small" variant={variant} disabled aria-describedby={id} aria-label={label}>{children}</Button><small id={id}>{reason}</small></span>;
}

/** A text link styled like the app's quiet links ("Open in Decisions ›"). */
export function LinkButton({ children, onClick, small = false, label }: { children: ReactNode; onClick?: () => void; small?: boolean; label?: string }) {
  return <button type="button" className={small ? "v-link v-link--sm" : "v-link"} onClick={onClick} aria-label={label} disabled={!onClick}>{children}</button>;
}

/** Shows within 100 ms of Add, while the field and button are disabled. */
export function WorkingLine({ children }: { children: ReactNode }) {
  return <p className="v-working" role="status"><i aria-hidden="true" />{children}</p>;
}

/** Saved / done / couldn't save, with an optional Undo that lasts 8 s. */
export function ResultLine({ children, error = false, onUndo, startedAt, extra }: { children: ReactNode; error?: boolean; onUndo?: () => void; startedAt?: number; extra?: ReactNode }) {
  const [left, setLeft] = useState(() => (startedAt ? Math.max(0, Math.ceil((UNDO_MS - (Date.now() - startedAt)) / 1000)) : 0));
  useEffect(() => {
    if (!onUndo || !startedAt) return;
    const id = window.setInterval(() => setLeft(Math.max(0, Math.ceil((UNDO_MS - (Date.now() - startedAt)) / 1000))), 250);
    return () => window.clearInterval(id);
  }, [onUndo, startedAt]);
  return (
    <div className={error ? "v-result v-result--err" : "v-result"} role="status">
      <span className="status-text" data-s={error ? "warn" : "ok"}><span className="status-text__glyph" aria-hidden="true">{error ? "▲" : "✓"}</span></span>
      <span>{children}</span>
      {onUndo && left > 0 && <><Button size="small" variant="plain" onClick={onUndo} aria-label={`Undo (${left} seconds left)`}>Undo</Button><span className="v-note" aria-hidden="true">{left} s</span></>}
      {extra}
    </div>
  );
}

/** "Check before saving": the parsed fields, then the page's actions. Chat shows this same card. */
export function PreviewCard({ kind, preview, sample, children, label }: { kind: string; preview: AddPreview; sample: boolean; children: ReactNode; label?: string }) {
  return (
    <section className="v-preview" aria-label={label ?? `Preview: ${kind}`}>
      <div className="v-preview__top"><span className="v-eyebrow">Check before saving · {kind}</span><span className="v-grow" />{sample && <DataTag kind="sample" />}</div>
      <h3>{preview.title}</h3>
      <dl className="v-kv">{preview.fields.map(([k, v]) => <div key={k} style={{ display: "contents" }}><dt>{k}</dt><dd>{v}</dd></div>)}</dl>
      {preview.note && <p className="v-note">{preview.note}</p>}
      <div className="v-actions">{children}</div>
    </section>
  );
}

/** "History · N changes ›" and its sheet (the 1111t takes sheet look). */
export function AddHistory({ adapter, title }: { adapter: AddAdapter; title: string }) {
  const [open, setOpen] = useState(false);
  const h = adapter.history();
  const list = valueOf(h);
  if (!list) return <span className="v-note">History <DataTag kind="wire" /></span>;
  return (
    <>
      <button type="button" className="take__hist" onClick={() => setOpen(true)}>History · {list.length} change{list.length === 1 ? "" : "s"} ›</button>
      <Dialog open={open} title={`${title} · history`} onCancel={() => setOpen(false)} className="t-sheet">
        <div className="t-sheet__bar">{h.state === "sample" && <DataTag kind="sample" />}<span className="t-grow" /><Button size="small" onClick={() => setOpen(false)}>Close</Button></div>
        <ul className="v-hist" aria-label={`${title} history`}>{list.map((x) => <li key={x.id}><span className="v-src">{x.when}</span><span className="v-inbox__t"><b>{x.text}</b><span>from {x.via}</span></span></li>)}</ul>
        <p className="t-note">Kept on this Mac. A change is undone from the page within 8 s; after that a correction is a new change.</p>
      </Dialog>
    </>
  );
}

export type ComposerProps = {
  /** Card title, e.g. "New goal or habit". */
  title: string;
  /** Kind for the preview eyebrow, e.g. "Habit". */
  kind: string;
  placeholder: string;
  hint: string;
  addLabel?: string;
  saveLabel?: string;
  working: string;
  adapter: AddAdapter;
  onSaved?: (saved: Saved, preview: AddPreview) => void;
  onUndone?: (id: string) => void;
  onOpen?: (route: Route) => void;
  /** Where it went, e.g. "Added to This week's habits." */
  savedText: (saved: Saved) => string;
};

/** design-1111v §5 composer: label, free text, mic, Add; then working → preview → saved with Undo → History. */
export function Composer({ title, kind, placeholder, hint, addLabel = "Add", saveLabel = "Save", working, adapter, onSaved, onUndone, onOpen, savedText }: ComposerProps) {
  const [state, send] = useReducer(flow, IDLE);
  const input = useRef<HTMLInputElement>(null);
  const id = useId();
  const sample = adapter.history().state === "sample";

  const read = useCallback(async (text: string) => {
    try { send({ type: "parsed", result: await adapter.parse(text) }); }
    catch { send({ type: "failed", message: "Couldn't read it: Amara isn't reachable. Your text is kept." }); }
  }, [adapter]);
  useEffect(() => { if (state.phase === "working") void read(state.text); }, [state, read]);
  useEffect(() => {
    if (state.phase !== "saving") return;
    const preview = state.preview;
    adapter.save(preview).then((result) => {
      send({ type: "saved", result, at: Date.now() });
      if (result.state !== "unwired") onSaved?.(result.value, preview);
    }, () => send({ type: "failed", message: "Couldn't save: Amara isn't reachable. Your text is kept." }));
  }, [state, adapter, onSaved]);
  useEffect(() => {
    if (state.phase !== "saved") return;
    const t = window.setTimeout(() => send({ type: "expire" }), Math.max(0, UNDO_MS - (Date.now() - state.at)));
    return () => window.clearTimeout(t);
  }, [state]);

  const undo = async () => {
    if (state.phase !== "saved") return;
    const sid = state.saved.id;
    await adapter.undo(sid);
    send({ type: "undone" });
    onUndone?.(sid);
  };
  const isBusy = busy(state);
  const submit = () => { send({ type: "submit" }); };
  const typing = state.phase === "idle" && state.text.length > 0;

  return (
    <Card title={title} accessory={<span className="pk-acc"><AddHistory adapter={adapter} title={title} /></span>}>
      <form className="v-composer" data-state={isBusy ? "working" : typing ? "typing" : "idle"} onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <label className="v-composer__label" htmlFor={id}>Type it the way you&apos;d say it</label>
        <div className="v-composer__row">
          <input ref={input} id={id} className="v-composer__input" placeholder={placeholder} value={state.phase === "saved" || state.phase === "undone" ? "" : state.text} maxLength={500} disabled={isBusy}
            onChange={(e) => send({ type: "type", text: e.target.value })} />
          <button type="button" className="v-mic" disabled aria-label="Dictate on this Mac (needs wiring: on-device transcription)" title="Needs wiring · voice is transcribed on this Mac only"><Mic aria-hidden="true" /></button>
          {adapter.wired
            ? <Button type="submit" size="small" variant="primary" disabled={isBusy || !state.text.trim()}>{addLabel}</Button>
            : <OffButton variant="primary" reason={`Needs wiring · ${adapter.command}`}>{addLabel}</OffButton>}
        </div>
        <p className="v-hint">{hint}</p>
      </form>
      {state.phase === "working" && <WorkingLine>{working}</WorkingLine>}
      {state.phase === "saving" && <WorkingLine>Saving…</WorkingLine>}
      {state.phase === "preview" && (
        <PreviewCard kind={kind} preview={state.preview} sample={sample}>
          <Button size="small" variant="primary" onClick={() => send({ type: "confirm" })}>{saveLabel}</Button>
          <Button size="small" onClick={() => { send({ type: "edit" }); input.current?.focus(); }}>Edit</Button>
          <Button size="small" variant="plain" onClick={() => send({ type: "cancel" })}>Cancel</Button>
        </PreviewCard>
      )}
      {state.phase === "ask" && (
        <section className="v-preview" aria-label={`Question: ${state.question}`}>
          <h3 style={{ fontSize: 13 }}>{state.question}</h3>
          {state.note && <p className="v-note">{state.note}</p>}
          <div className="v-actions">
            {state.options.map((o) => <Button key={o} size="small" onClick={() => send({ type: "answer", option: o })}>{o}</Button>)}
            <Button size="small" variant="plain" onClick={() => { send({ type: "edit" }); input.current?.focus(); }}>Edit text</Button>
          </div>
        </section>
      )}
      {state.phase === "elsewhere" && <ResultLine extra={onOpen && <LinkButton small onClick={() => onOpen(state.link)}>{state.linkLabel}</LinkButton>}>{state.message}</ResultLine>}
      {state.phase === "saved" && <ResultLine onUndo={() => { void undo(); }} startedAt={state.at} extra={state.saved.also && onOpen && <LinkButton small onClick={() => onOpen(state.saved.also!.link)}>Also shows in {state.saved.also.label}</LinkButton>}>{savedText(state.saved)}{sample ? " (sample data: preview only)" : ""}</ResultLine>}
      {state.phase === "undone" && <ResultLine>Undone. &ldquo;{state.title}&rdquo; was removed.</ResultLine>}
      {state.phase === "error" && <ResultLine error extra={adapter.wired && <Button size="small" onClick={submit}>Try again</Button>}>{state.message}</ResultLine>}
    </Card>
  );
}
