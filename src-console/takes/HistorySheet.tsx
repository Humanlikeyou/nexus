import { useState } from "react";
import { Button, DataTag, Dialog } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { TAKE_PAGE_LABEL } from "../amaraTake/takeView";
import type { TakePage } from "../amaraTake/types";
import type { Answer, HistoryEntry, HistoryFilter, TakesAdapter } from "./adapters";
import { Rich, VerdictChip } from "./parts";
import { useTakeLogVersion } from "./takeLog";

const ANSWER_WORD = (a: Answer) => a.state === "done" ? (a.source === "detected" ? "✓ Done · detected" : "✓ Done") : a.state === "skipped" ? "× Skipped" : a.state === "not_today" ? "Not today" : a.state === "unknown" ? "– Unknown" : "◔ Waiting";

/** "History · N takes ›" (t4): every take, suggestion and answer for this page, kept on this Mac for 1 year. */
export function HistorySheet({ page, open, onClose, adapter }: { page: TakePage; open: boolean; onClose: () => void; adapter: TakesAdapter }) {
  const [filter, setFilter] = useState<HistoryFilter>("all");
  const [q, setQ] = useState("");
  const [forgetting, setForgetting] = useState(false);
  const [confirm, setConfirm] = useState<HistoryEntry | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [, bump] = useState(0);
  useTakeLogVersion(); // re-render when the take log loads or a take is forgotten
  const w = adapter.history(page, filter, q);
  const list = valueOf(w);
  const all = valueOf(adapter.history(page, "all", ""));
  const count = (f: HistoryFilter) => (all ? all.filter((h) => f === "all" || (f === "suggestion" ? h.suggestion : h.kind === "followup")).length : 0);
  const title = `${TAKE_PAGE_LABEL[page]} · Amara's takes`;
  const weeks = list ? [...new Set(list.map((h) => h.week))] : [];
  const forget = async (h: HistoryEntry) => { const r = await adapter.forget(page, h.id); setConfirm(null); setNote(r.state === "unwired" ? "Forgetting needs wiring, so nothing changed." : `${r.state === "sample" ? "Sample data: " : ""}Forgot the ${h.day} take. Its answer stays in the results; the next take says so.`); bump((n) => n + 1); };
  return (
    <Dialog open={open} title={title} onCancel={onClose} className="t-sheet">
      <div className="t-sheet__bar">
        {w.state === "sample" && <DataTag kind="sample" />}
        <span className="t-grow" />
        <Button size="small" onClick={onClose}>Close</Button>
      </div>
      {!list ? <p className="t-empty">Take history <DataTag kind="wire" />. Once the take log exists, every day&apos;s take, suggestion and answer collects here, newest first. Kept on this Mac only.</p> : (
        <>
          <div className="t-sheet__tools">
            <div className="pk-chips" role="group" aria-label="Show">
              {([["all", "All"], ["suggestion", "With a suggestion"], ["followup", "Follow-ups"]] as Array<[HistoryFilter, string]>).map(([f, word]) => (
                <button key={f} type="button" className="pk-chip ci-chip" aria-pressed={filter === f} data-on={filter === f || undefined} onClick={() => setFilter(f)}>{word} {count(f)}</button>
              ))}
            </div>
            <label className="t-search"><span className="ui-visually-hidden">Search takes</span><input type="search" placeholder="Search takes" value={q} onChange={(e) => setQ(e.target.value)} /></label>
          </div>
          <div className="t-sheet__scroll">
            {list.length === 0 ? <p className="t-empty">{all && all.length ? "No takes match." : "No takes yet. Each day's take, suggestion and your answer will collect here, newest first. Kept on this Mac only."}</p> : weeks.map((wk) => (
              <div key={wk} className="t-hist">
                <h3 className="t-week">{wk}</h3>
                {list.filter((h) => h.week === wk).map((h) => (
                  <article key={h.id} className="t-hist__day">
                    <time><b>{h.day}</b>{h.weekday}</time>
                    <div>
                      <p><Rich text={h.text} /></p>
                      <div className="t-hist__rec">
                        {h.verdict && <VerdictChip v={h.verdict} short />}
                        {h.suggestion && <><b>{h.suggestion.title}</b><span>{ANSWER_WORD(h.suggestion.answer)}{h.suggestion.answer.reason === "no_time" ? " · no time" : ""}</span></>}
                        {forgetting && <button type="button" className="t-small" aria-label={`Forget the ${h.day} take`} onClick={() => setConfirm(h)}>Forget…</button>}
                      </div>
                    </div>
                  </article>
                ))}
              </div>
            ))}
          </div>
        </>
      )}
      <div className="t-sheet__foot">
        <div className="pk-row"><div className="pk-row__main"><span>Amara remembers every take, suggestion and answer on this Mac</span><small>She refers back to them (&ldquo;On Oct 7 I suggested…&rdquo;) from stored rows, not recall. Kept 1 year, then summarized into Memory. Never leaves this Mac.</small></div>
          {list && <Button size="small" aria-pressed={forgetting} onClick={() => setForgetting((f) => !f)}>{forgetting ? "Done" : "Forget a take…"}</Button>}</div>
        <p className="t-note">Settings › Memory › Takes controls this for every page.</p>
        {note && <p className="t-note" role="status">{note}</p>}
      </div>
      <Dialog open={confirm !== null} title={confirm ? `Forget the ${confirm.day} take?` : "Forget this take?"} destructive confirmLabel="Forget" onConfirm={() => { if (confirm) void forget(confirm); }} onCancel={() => setConfirm(null)}>
        Its text is blanked on this Mac. Your answer that day stays in the results unless you forget answers too in Settings › Memory › Takes.
      </Dialog>
    </Dialog>
  );
}
