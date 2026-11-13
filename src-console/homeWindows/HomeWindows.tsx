/** design-1111w: Jarvis windows stage on Home. Windows stay dark in both themes. */
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { AmaraLens } from "../components/AmaraLens/AmaraLens";
import type { AmaraVoiceState } from "../hooks/useAmaraVoice";
import { sampleAdaptersEnabled } from "../lib/sampleAdapters";
import { useNow } from "../shell/useNow";
import { Button, DataTag } from "../ui";
import { addAdapter } from "../adds/adapters";
import { PreviewCard, ResultLine } from "../adds/Composer";
import type { AddPreview, Saved } from "../adds/flow";
import { entryById } from "./catalog";
import { canComment, shouldSpeakComment } from "./intents";
import { layoutSlots, shortTitle, toastLabel } from "./reducer";
import { SampleBody, StateBody } from "./SampleBodies";
import { homeWindowsAdapter, liveEmbedding } from "./adapters";
import { useHomeWindowsState } from "./store";
import {
  closeAll, expandWindow, focusWindow, handleWindowPrompt, popOutWindow,
  undoWindows, useHomeWindowsClock, useWindowKeys,
} from "./useHomeWindows";
import { homeWindowsStore } from "./store";
import type { HomeWindow, StagePhase } from "./types";
import "../adds/v1111.css";
import "./homeWindows.css";

const clock = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", hour12: true });

function lensState(phase: StagePhase, reading: boolean): AmaraVoiceState {
  if (reading) return "speaking";
  if (phase === "hearing") return "listening";
  if (phase === "understood" || phase === "exiting" || phase === "entering") return "thinking";
  return "idle";
}

function railLabel(phase: StagePhase, heard: string | null, reading: boolean): { state: string; sub?: string } {
  if (phase === "hearing") return { state: "Hearing…", sub: heard ?? undefined };
  if (phase === "understood") return { state: "Opening", sub: heard ?? undefined };
  if (phase === "exiting" || phase === "entering") return { state: "Opening" };
  if (phase === "closing" || phase === "returning") return { state: "Closing" };
  if (reading) return { state: "Reading…" };
  return { state: "Ready" };
}

function IconPop() {
  return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>;
}
function IconExp() {
  return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>;
}
function IconX() {
  return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>;
}
function IconMic() {
  return <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><rect x="9" y="3" width="6" height="11" rx="3" /><path d="M5 11a7 7 0 0 0 14 0M12 18v3" /></svg>;
}

/** Catalog comments may contain <b> only. */
export function RichComment({ html }: { html: string }) {
  const parts = html.split(/(<b>.*?<\/b>)/g);
  return (
    <>
      {parts.map((p, i) => {
        const m = /^<b>(.*?)<\/b>$/.exec(p);
        return m ? <b key={i}>{m[1]}</b> : <span key={i}>{p}</span>;
      })}
    </>
  );
}

function Wave() {
  return (
    <span className="w-wave" aria-hidden="true">
      {Array.from({ length: 13 }, (_, i) => <i key={i} style={{ height: `${3 + ((i * 7) % 9)}px` }} />)}
    </span>
  );
}

function PdfDateAdd({ hint }: { hint: string }) {
  const [phase, setPhase] = useState<"idle" | "preview" | "saved">("idle");
  const [preview, setPreview] = useState<AddPreview | null>(null);
  const [saved, setSaved] = useState<Saved | null>(null);
  const [savedAt, setSavedAt] = useState(0);
  const [unwired, setUnwired] = useState(false);
  const sample = sampleAdaptersEnabled();
  const start = () => {
    const a = addAdapter("date");
    void a.parse(`Lease notice due ${hint}`).then((r) => {
      setUnwired(r.state === "unwired");
      if (r.state === "unwired") { setPhase("preview"); setPreview({ title: "Lease notice", fields: [["Date", hint], ["Lane", "Personal"]], note: "Needs wiring to save to Dates & deadlines." }); return; }
      if (r.value.kind === "preview") { setPreview(r.value.preview); setPhase("preview"); }
    });
  };
  const confirm = () => {
    if (!preview) return;
    void addAdapter("date").save(preview).then((r) => {
      setSavedAt(Date.now());
      if (r.state === "unwired") { setSaved({ id: "unwired", title: preview.title }); setPhase("saved"); return; }
      setSaved(r.value); setPhase("saved");
    });
  };
  const undo = () => {
    if (saved && saved.id !== "unwired") void addAdapter("date").undo(saved.id);
    setPhase("idle"); setSaved(null); setPreview(null);
  };
  if (phase === "preview" && preview) {
    return (
      <PreviewCard kind="Date" preview={preview} sample={sample} label="Preview: add to Dates & deadlines">
        <Button size="small" onClick={confirm}>{sample ? "Save date" : "Save date"}</Button>
        <Button size="small" variant="secondary" onClick={() => setPhase("idle")}>Cancel</Button>
        {unwired && <DataTag kind="wire" />}
      </PreviewCard>
    );
  }
  if (phase === "saved" && saved) {
    return <ResultLine onUndo={undo} startedAt={savedAt}>{`Added “${saved.title}” to Dates & deadlines.`}{sample ? " (sample data: preview only)" : ""}</ResultLine>;
  }
  return <Button size="small" onClick={start}>Add {hint} to Dates & deadlines…</Button>;
}

function Caption({ win, compact }: { win: HomeWindow; compact: boolean }) {
  if (win.comment.phase === "off" || win.comment.phase === "paused") return null;
  if (!canComment(win) && win.comment.phase !== "reading") return null;
  const speak = shouldSpeakComment(win);
  const replay = () => { if (win.comment.text) void homeWindowsAdapter().speak(win.comment.text); };
  const entry = entryById(win.catalogId);
  return (
    <div className="w-cap" role="status" aria-live="polite" data-compact={compact || undefined}>
      <span className="w-cap__ember" aria-hidden="true" />
      <div className="w-cap__text">
        {win.comment.phase === "reading" || (win.contentState === "loading" && win.comment.phase !== "ready") ? (
          <p className="w-working"><i aria-hidden="true" />Reading the {win.kind === "chart" ? "chart" : win.kind === "pdf" ? "PDF" : win.kind === "video" ? "video" : "page"}…</p>
        ) : win.comment.text ? (
          <p><b>Amara:</b> <RichComment html={win.comment.text} /></p>
        ) : null}
        {win.kind === "pdf" && win.comment.phase === "ready" && entry.pdfBullets && (
          <ul>{entry.pdfBullets.map((b) => <li key={b}>{b}</li>)}</ul>
        )}
        {win.kind === "pdf" && win.comment.phase === "ready" && entry.pdfDateHint && <PdfDateAdd hint={entry.pdfDateHint} />}
        <div className="w-cap__meta">
          {speak && <><Wave /><span>Spoken</span><Button variant="plain" size="small" onClick={replay}>Replay</Button></>}
          <span>Local · light tier · from what's on screen</span>
          <DataTag kind="sample" />
        </div>
      </div>
    </div>
  );
}

function WindowCard({ win, slot, focused, leaving, onSignIn, onRetry }: {
  win: HomeWindow; slot: "primary" | "a" | "b"; focused: boolean; leaving: boolean;
  onSignIn: () => void; onRetry: () => void;
}) {
  const titleRef = useRef<HTMLSpanElement>(null);
  useEffect(() => { if (focused) titleRef.current?.focus(); }, [focused, win.id]);
  const name = shortTitle(win.title);
  return (
    <section
      className="w-win"
      aria-label={`${name} window`}
      data-slot={slot}
      data-focus={focused || undefined}
      data-expanded={win.expanded || undefined}
      data-leaving={leaving || undefined}
      data-kind={win.kind}
      data-id={win.id}
      onPointerDown={() => { if (!focused) focusWindow(win.id); }}
    >
      <header className="w-bar">
        <span className="w-fav" style={{ background: win.fav.color }} aria-hidden="true">{win.fav.letter}</span>
        <span className="w-bar__title" ref={titleRef} tabIndex={-1}>{win.title}</span>
        <span className="w-bar__src">{win.source}</span>
        {win.signed === "in" && <span className="w-chip" data-k="in">✓ Signed in</span>}
        {win.signed === "out" && <span className="w-chip">Signed out</span>}
        <span className="w-grow" />
        <button type="button" className="w-ib" aria-label={`Pop out ${name} into its own window`} onClick={(e) => { e.stopPropagation(); popOutWindow(win.id); }}><IconPop /></button>
        <button type="button" className="w-ib" aria-label={`${win.expanded ? "Restore" : "Expand"} ${name}`} onClick={(e) => { e.stopPropagation(); expandWindow(win.id); }}><IconExp /></button>
        <button type="button" className="w-ib" aria-label={`Close ${name} (Esc)`} aria-keyshortcuts="Escape" onClick={(e) => { e.stopPropagation(); homeWindowsStore.dispatch({ type: "close", target: win.id, at: Date.now() }); }}><IconX /></button>
      </header>
      <div className="w-body">
        <StateBody
          state={win.contentState}
          host={win.source}
          onSignIn={onSignIn}
          onPopOut={() => popOutWindow(win.id)}
          onRetry={onRetry}
        />
        {sampleAdaptersEnabled() ? <SampleBody win={win} /> : win.contentState === "ready" && !liveEmbedding() ? <div className="w-center"><h3>Needs wiring</h3><p>A live page needs a webview from Claude before it can open here.</p><DataTag kind="wire" /></div> : null}
      </div>
      <Caption win={win} compact={slot !== "primary"} />
    </section>
  );
}

function CommandBar({ onAsk }: { onAsk?: (text: string) => void }) {
  const [text, setText] = useState("");
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const q = text.trim();
    if (!q) return;
    setText("");
    if (handleWindowPrompt(q, "chat")) return;
    onAsk?.(q);
  };
  return (
    <form className="w-cmd" onSubmit={submit}>
      <button type="button" className="w-cmd__mic" aria-label="Talk to Amara" disabled><IconMic /></button>
      <input className="w-cmd__field" value={text} onChange={(e) => setText(e.target.value)} placeholder="Ask Amara, or say “close the chart”" aria-label="Ask Amara" />
      <span className="w-kbd" aria-hidden="true">Esc</span>
      <span style={{ fontSize: 11 }}>closes the focused window</span>
    </form>
  );
}

export function HomeWindowsStage({ onAsk }: { onAsk?: (text: string) => void }) {
  useHomeWindowsClock();
  useWindowKeys();
  const state = useHomeWindowsState();
  const now = useNow(30_000);
  if (state.phase === "home" && !state.toast && !state.undo) return null;
  const onStage = state.windows.filter((w) => !w.poppedOut);
  const popped = state.windows.filter((w) => w.poppedOut);
  const slots = layoutSlots(state.windows, state.focusedId);
  const expanded = onStage.some((w) => w.expanded);
  const reading = state.windows.some((w) => w.comment.phase === "reading");
  const label = railLabel(state.phase, state.heard?.chip ?? state.heard?.query ?? null, reading);
  const hidden = expanded ? Math.max(0, onStage.length - 1) : 0;
  const showArea = state.phase !== "hearing" && state.phase !== "understood" && state.phase !== "exiting" && state.phase !== "home" && state.phase !== "returning";
  const showCmd = showArea || state.phase === "closing";

  return (
    <div className="w-stage" data-phase={state.phase} data-testid="home-windows-stage">
      <header className="w-rail">
        <div className="w-rail__center">
          <div className="w-orb" aria-hidden="true"><AmaraLens state={lensState(state.phase, reading)} size={320} decorative /></div>
          <span className="w-rail__state" role="status">{label.state}{label.sub && <small>{label.sub}</small>}</span>
        </div>
        <div className="w-rail__right">
          {hidden > 0 && <span>+{hidden} hidden</span>}
          {popped.length > 0 && <span className="w-popnote">{popped.length} popped out (counts toward 3)</span>}
          {state.windows.length > 0 && <span>Windows <b>{state.windows.length}</b> of 3</span>}
          {state.windows.length > 0 && <button type="button" className="w-pill" onClick={closeAll}>Close all</button>}
          <span><b>{clock.format(now)}</b></span>
        </div>
      </header>
      {(state.phase === "hearing" || state.phase === "understood" || state.phase === "exiting") && state.heard && (
        <div className="w-area" data-n="1">
          <div className="w-heard">
            <span className="w-heard__q">{state.heard.query}</span>
            {state.phase !== "hearing" && state.heard.chip && <span className="w-heard__chip">{state.heard.chip}</span>}
          </div>
        </div>
      )}
      {showArea && (
        <div className="w-area" data-n={Math.max(1, onStage.length)} data-expanded={expanded || undefined}>
          {slots.map(({ id, slot }) => {
            const win = state.windows.find((w) => w.id === id);
            if (!win) return null;
            return (
              <WindowCard
                key={win.id}
                win={win}
                slot={slot}
                focused={state.focusedId === win.id}
                leaving={state.closingId === win.id}
                onSignIn={() => homeWindowsStore.dispatch({ type: "setContent", id: win.id, contentState: "signing_in" })}
                onRetry={() => {
                  if (!sampleAdaptersEnabled()) { homeWindowsStore.dispatch({ type: "setContent", id: win.id, contentState: "unwired" }); return; }
                  homeWindowsStore.dispatch({ type: "setContent", id: win.id, contentState: "loading" });
                  window.setTimeout(() => homeWindowsStore.dispatch({ type: "setContent", id: win.id, contentState: "ready" }), 400);
                }}
              />
            );
          })}
        </div>
      )}
      {state.phase === "returning" && <div className="w-area" data-n="1" />}
      {showCmd && <CommandBar onAsk={onAsk} />}
      {state.toast && (
        <div className="w-toast" role="status">
          {toastLabel(state.toast)}
          <Button variant="secondary" size="small" onClick={undoWindows}>Undo</Button>
        </div>
      )}
    </div>
  );
}

export function landingInert(phase: StagePhase): boolean {
  return phase === "entering" || phase === "windows" || phase === "closing";
}

export function HomeShell({ children, hidden }: { children: ReactNode; hidden: boolean }) {
  return <div inert={hidden || undefined} aria-hidden={hidden || undefined}>{children}</div>;
}
