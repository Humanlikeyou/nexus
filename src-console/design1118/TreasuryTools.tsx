import { useState } from "react";
import type { commands } from "../bindings.gen";
import type { TreasuryCsvMapping_Deserialize, TreasuryImportPreview } from "../bindings.gen";
import { Button } from "../ui";
import type { TreasuryStream } from "../business/treasury";
import "./design1118.css";

/**
 * Treasury actions (decision 0042). Only existing commands: treasury_stream_add, treasury_import_preview /
 * treasury_import_pick (with the column mapping), treasury_attention_set, treasury_flags_ack,
 * treasury_monitor_run_now, treasury_review_build. Nothing here sends anything. None of these commands
 * logs History, so there is no Undo; each action ends in a plain result line and the lists reload.
 */
export type TreasuryToolCalls = Pick<
  typeof commands,
  "treasuryStreamAdd" | "treasuryImportPreview" | "treasuryImportPick" | "treasuryAttentionSet" | "treasuryFlagsAck" | "treasuryMonitorRunNow" | "treasuryReviewBuild"
>;
export interface TreasuryTools {
  calls: TreasuryToolCalls;
  /** Ledger ventures a stream can belong to; null when there is no readable ledger. */
  ventures: { id: string; name: string }[] | null;
  /** The complete month (YYYY-MM) a review is built for. */
  reviewMonth: string;
  /** Reload the page's lists after a write. */
  onChanged: () => void;
}

const ERRORS: Record<string, string> = {
  cancelled: "No file was chosen, so nothing was changed.",
  busy: "Another Treasury job is running. Try again in a moment.",
  invalid_input: "Check the fields and try again.",
  unknown_stream: "That stream no longer exists.",
  unknown_venture: "That venture is not in the business ledger.",
  ledger_missing: "There is no business ledger yet, so a stream cannot be added.",
  ledger_invalid: "The business ledger could not be read.",
  bad_header: "A column name you gave is not in the file's header row.",
  bad_file: "That file could not be read.",
  too_large: "That file is too large.",
  too_many_rows: "That file has too many rows.",
  bad_encoding: "That file is not plain text (UTF-8).",
  unknown_adapter: "That file format is not supported.",
};
export const treasuryError = (e: unknown) => {
  const code = typeof e === "string" ? e : e instanceof Error ? e.message : "";
  return ERRORS[code] ?? "That didn't work. Nothing was changed.";
};
const ROW_CODE: Record<string, string> = {
  bad_date: "date not understood", bad_amount: "amount not understood", bad_currency: "currency not understood",
  short_row: "too few columns", wide_row: "too many columns",
};
const STREAM_TYPES: [string, string][] = [
  ["product_sales", "Product sales"], ["digital_product", "Digital product"], ["investment", "Investment"], ["affiliate", "Affiliate"], ["other", "Other"],
];
const today = () => { const d = new Date(); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`; };

interface MapForm { date: string; amount: string; currencyCol: string; defaultCur: string; ref: string; fmt: string }
const EMPTY_MAP: MapForm = { date: "", amount: "", currencyCol: "", defaultCur: "", ref: "", fmt: "ymd" };
/** The column mapping, or null until the required columns are named (and a currency is chosen one way). */
export function toMapping(f: MapForm): TreasuryCsvMapping_Deserialize | null {
  if (!f.date.trim() || !f.amount.trim() || (!f.currencyCol.trim() && !f.defaultCur)) return null;
  return {
    dateColumn: f.date.trim(), amountColumn: f.amount.trim(),
    currencyColumn: f.currencyCol.trim() || null, defaultCurrency: f.defaultCur || null,
    refColumn: f.ref.trim() || null, dateFormat: f.fmt,
  };
}

export function TreasuryActions({ tools, streams, say }: { tools: TreasuryTools; streams: TreasuryStream[]; say: (text: string) => void }) {
  const { calls, ventures, reviewMonth, onChanged } = tools;
  const [busy, setBusy] = useState<string | null>(null);
  const run = async (key: string, job: () => Promise<void>) => {
    if (busy) return;
    setBusy(key);
    try { await job(); } catch (e) { say(treasuryError(e)); } finally { setBusy(null); }
  };

  // Add stream
  const [venture, setVenture] = useState(""), [sname, setSname] = useState(""), [stype, setStype] = useState("product_sales");
  const addStream = () => run("add", async () => {
    const s = await calls.treasuryStreamAdd(venture, sname.trim(), stype, null, null);
    setSname(""); say(`Added the stream ${s.name}.`); onChanged();
  });

  // Import with preview
  const [impStream, setImpStream] = useState(""), [map, setMap] = useState<MapForm>(EMPTY_MAP);
  const [preview, setPreview] = useState<TreasuryImportPreview | null>(null);
  const mapping = toMapping(map);
  const edit = (patch: Partial<MapForm>) => { setMap((m) => ({ ...m, ...patch })); setPreview(null); };
  const doPreview = () => run("preview", async () => {
    if (!mapping) return;
    const p = await calls.treasuryImportPreview(impStream, "generic", mapping);
    setPreview(p);
    say(`Preview only, nothing is saved yet: ${p.rowsSeen} rows read, ${p.wouldAdd} would be added, ${p.wouldSkip} already there, ${p.failed} could not be read.`);
  });
  const doImport = () => run("import", async () => {
    if (!mapping || !preview) return;
    const r = await calls.treasuryImportPick(impStream, "generic", mapping);
    setPreview(null);
    say(`Imported: ${r.added} added, ${r.skipped} already there, ${r.failed} could not be read.`); onChanged();
  });

  // Log time
  const [tStream, setTStream] = useState(""), [day, setDay] = useState(today), [mins, setMins] = useState("");
  const minsN = Number(mins);
  const minsOk = mins.trim() !== "" && Number.isInteger(minsN) && minsN >= 0 && minsN <= 1440;
  const logTime = () => run("time", async () => {
    await calls.treasuryAttentionSet(tStream, day, minsN);
    setMins(""); say(`Logged ${minsN} minutes for ${day}. A day you log again is replaced.`); onChanged();
  });

  // Check now / build review
  const check = () => run("check", async () => {
    const r = await calls.treasuryMonitorRunNow();
    say(`Checked just now: ${r.flags} to look at, ${r.newFlags} new, ${r.cleared} cleared.`); onChanged();
  });
  const build = () => run("review", async () => {
    const r = await calls.treasuryReviewBuild(reviewMonth);
    say(r.state === "ok" ? `The review for ${r.month} is ready below.`
      : r.state === "hidden" ? `The review for ${r.month} was held back because its numbers did not match the ledger.`
        : `There is no income data for ${r.month}, so there is no review.`);
    onChanged();
  });

  const noStreams = streams.length === 0;
  return (
    <section className="d18-card" aria-labelledby="tr-tools" data-testid="tr-tools">
      <div className="d18-map__head"><h2 id="tr-tools" className="d18-h">Add and update</h2></div>
      <div className="d18-actions">
        <Button size="small" variant="secondary" disabled={busy !== null} onClick={() => { void check(); }}>Check now</Button>
        <Button size="small" variant="secondary" disabled={busy !== null} onClick={() => { void build(); }}>Build review for {reviewMonth}</Button>
      </div>

      <h3 className="d18-note"><b>Add a stream</b></h3>
      {ventures === null || ventures.length === 0
        ? <p className="d18-note">Needs wiring · a stream belongs to a venture in the business ledger, and none is readable yet.</p>
        : (
          <div className="d18-actions">
            <label className="d18-note" htmlFor="tr-venture">Venture</label>
            <select id="tr-venture" value={venture} onChange={(e) => setVenture(e.target.value)}>
              <option value="">Choose…</option>{ventures.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
            </select>
            <label className="d18-note" htmlFor="tr-sname">Name</label>
            <input id="tr-sname" value={sname} maxLength={80} onChange={(e) => setSname(e.target.value)} />
            <label className="d18-note" htmlFor="tr-stype">Type</label>
            <select id="tr-stype" value={stype} onChange={(e) => setStype(e.target.value)}>{STREAM_TYPES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>
            <Button size="small" disabled={busy !== null || !venture || !sname.trim()} onClick={() => { void addStream(); }}>Add stream</Button>
          </div>
        )}

      <h3 className="d18-note"><b>Import a file</b></h3>
      {noStreams ? <p className="d18-note">Add a stream first. An import belongs to one stream.</p> : (
        <>
          <p className="d18-note">Pick a CSV file from this Mac and name its columns. You see what would be read before anything is saved.</p>
          <div className="d18-actions">
            <label className="d18-note" htmlFor="tr-imp-stream">Stream</label>
            <select id="tr-imp-stream" value={impStream} onChange={(e) => { setImpStream(e.target.value); setPreview(null); }}>
              <option value="">Choose…</option>{streams.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
            </select>
            <label className="d18-note" htmlFor="tr-m-date">Date column</label>
            <input id="tr-m-date" value={map.date} onChange={(e) => edit({ date: e.target.value })} />
            <label className="d18-note" htmlFor="tr-m-amount">Amount column</label>
            <input id="tr-m-amount" value={map.amount} onChange={(e) => edit({ amount: e.target.value })} />
            <label className="d18-note" htmlFor="tr-m-curcol">Currency column (optional)</label>
            <input id="tr-m-curcol" value={map.currencyCol} onChange={(e) => edit({ currencyCol: e.target.value })} />
            <label className="d18-note" htmlFor="tr-m-cur">Currency when there is no column</label>
            <select id="tr-m-cur" value={map.defaultCur} onChange={(e) => edit({ defaultCur: e.target.value })}>
              <option value="">None</option><option value="JPY">JPY</option><option value="USD">USD</option>
            </select>
            <label className="d18-note" htmlFor="tr-m-ref">Order id column (optional)</label>
            <input id="tr-m-ref" value={map.ref} onChange={(e) => edit({ ref: e.target.value })} />
            <label className="d18-note" htmlFor="tr-m-fmt">Date order</label>
            <select id="tr-m-fmt" value={map.fmt} onChange={(e) => edit({ fmt: e.target.value })}>
              <option value="ymd">Year, month, day</option><option value="mdy">Month, day, year</option><option value="dmy">Day, month, year</option>
            </select>
            <Button size="small" variant="secondary" disabled={busy !== null || !impStream || !mapping} onClick={() => { void doPreview(); }}>Choose file and preview</Button>
          </div>
          {preview && (
            <div data-testid="tr-preview">
              <p className="d18-note">{preview.rowsSeen} rows read · {preview.wouldAdd} would be added · {preview.wouldSkip} already there · {preview.failed} could not be read</p>
              {preview.errors.length > 0 && (
                <ul className="d18-clist" aria-label="Rows that could not be read">
                  {preview.errors.map((er, i) => <li key={`${er.row}-${i}`}><div><b>Row {er.row}</b><small>{ROW_CODE[er.code] ?? er.code}</small></div></li>)}
                </ul>
              )}
              <div className="d18-actions">
                <Button size="small" disabled={busy !== null || preview.wouldAdd === 0} onClick={() => { void doImport(); }}>Import {preview.wouldAdd} rows</Button>
                <Button size="small" variant="plain" onClick={() => { setPreview(null); say("Import cancelled. Nothing was saved."); }}>Cancel</Button>
              </div>
              <p className="d18-note">You will be asked to choose the same file again to confirm.</p>
            </div>
          )}
        </>
      )}

      <h3 className="d18-note"><b>Log time</b></h3>
      {noStreams ? <p className="d18-note">Add a stream first. Time is logged against one stream.</p> : (
        <div className="d18-actions">
          <label className="d18-note" htmlFor="tr-t-stream">Stream</label>
          <select id="tr-t-stream" value={tStream} onChange={(e) => setTStream(e.target.value)}>
            <option value="">Choose…</option>{streams.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
          </select>
          <label className="d18-note" htmlFor="tr-t-day">Day</label>
          <input id="tr-t-day" type="date" value={day} onChange={(e) => setDay(e.target.value)} />
          <label className="d18-note" htmlFor="tr-t-mins">Minutes</label>
          <input id="tr-t-mins" inputMode="numeric" value={mins} onChange={(e) => setMins(e.target.value)} />
          <Button size="small" disabled={busy !== null || !tStream || !day || !minsOk} onClick={() => { void logTime(); }}>Log time</Button>
        </div>
      )}
    </section>
  );
}
