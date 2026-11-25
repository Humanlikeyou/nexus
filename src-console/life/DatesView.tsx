import { useSvgTextScale } from "../ui/useSvgTextScale";
import { useEffect, useState } from "react";
import type { Route } from "../app/routes";
import { addAdapter } from "../adds/adapters";
import { announceAdds, inTauri, realInvoke, useStoresTick } from "../lib/localStores";
import { Composer, LinkButton, ResultLine } from "../adds/Composer";
import type { AddPreview, Saved } from "../adds/flow";
import { valueOf } from "../lib/sampleAdapters";
import { addDays, jstToday, lifeDateFrom, PERSONAL_DEFAULTS, type LifeDate, type LifeDateWire, type LifeLane, type PersonalAdapter } from "../personal/adapters";
import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import { Button, Card, DataTag, Dialog, PageHero, Select, StatusText, TextField } from "../ui";
import { placeLabels } from "./timeline";
import "../ui/PanelKit.css";

const LANES: Array<{ lane: LifeLane; name: string; cls: string; sub?: string }> = [
  { lane: "personal", name: "Personal", cls: "p" },
  { lane: "residence", name: "Japan · residence", cls: "j" },
  { lane: "enterprise", name: "Enterprise", cls: "b", sub: "read-only · from Enterprise" },
];
const LANE_NAME: Record<LifeLane, string> = { personal: "Personal", residence: "Japan · residence", enterprise: "Enterprise" };
const MONTHS = ["Nov", "Dec", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct"];
const W = 610, L = 110, TOP = 21, ROW = 26;
const xOf = (months: number) => (m: number) => L + (m / months) * (W - L - 13);
const MONTH_SHORT = new Intl.DateTimeFormat("en-US", { month: "short", timeZone: "UTC" });
/** Month tick labels for a span starting at `from` (YYYY-MM-DD): every month up to 12, else every 3rd/6th/12th with the year. */
export function rangeTicks(from: string, months: number): Array<{ m: number; label: string }> {
  const step = months <= 12 ? 1 : months <= 36 ? 3 : months <= 72 ? 6 : 12;
  const start = new Date(`${from.slice(0, 7)}-01T00:00:00Z`);
  const out: Array<{ m: number; label: string }> = [];
  for (let m = 0; m < months; m += step) {
    const d = new Date(Date.UTC(start.getUTCFullYear(), start.getUTCMonth() + m, 1));
    out.push({ m, label: months <= 12 && d.getUTCMonth() !== 0 ? MONTH_SHORT.format(d) : `${MONTH_SHORT.format(d)} ${String(d.getUTCFullYear()).slice(2)}` });
  }
  return out;
}

/** Where an Enterprise date is edited (it is read-only here). */
const enterpriseLink = (d: LifeDate): Route => (/taiyo skin launch/i.test(d.title) ? { id: "enterprise", view: "ventures/taiyo-skin" } : /tax/i.test(d.title) ? { id: "enterprise", view: "finances" } : { id: "enterprise", view: "ventures" });

/** design-1111v LaneTimeline: three lanes, labels that never overlap (placeLabels). 12 months by default (labels Nov–Oct as designed);
 *  a range (PLAN-0055) passes its own span in months and its tick labels. */
export function LaneTimeline({ dates, highlight, months = 12, ticks, span = "the next 12 months" }: { dates: LifeDate[]; highlight?: Set<string>; months?: number; ticks?: Array<{ m: number; label: string }>; span?: string }) {
  const textScale = useSvgTextScale();
  const x = xOf(months);
  const marks = ticks ?? MONTHS.map((label, m) => ({ m, label }));
  const lanes = LANES.map((l) => {
    const pts = dates.filter((d) => d.lane === l.lane);
    const placed = placeLabels(pts.map((d) => ({ id: d.id, x: x(d.month), label: d.short ?? d.title, date: d.date })), { width: W, rows: 2 });
    const rows = Math.max(1, ...placed.map((p) => p.row + 1));
    return { ...l, pts, placed, h: Math.max(68, 21 + rows * ROW + 13) };
  });
  const tops = lanes.reduce<number[]>((acc, l, i) => [...acc, i === 0 ? TOP : acc[i - 1] + lanes[i - 1].h], []);
  const bottom = tops[tops.length - 1] + lanes[lanes.length - 1].h;
  return (
    <svg ref={textScale} className="v-tl" viewBox={`0 0 ${W} ${bottom + 8}`} role="img" aria-label={`Dates over ${span} in three lanes: ${dates.map((d) => `${d.title}, ${d.date}, ${LANE_NAME[d.lane]}`).join("; ")}`}>
      {marks.map((t) => <g key={t.m}><line className="grid" x1={x(t.m)} x2={x(t.m)} y1={TOP - 8} y2={bottom} /><text x={x(t.m) + 5} y={TOP - 8}>{t.label}</text></g>)}
      {!ticks && <line className="today" x1={x(0)} x2={x(0)} y1={TOP - 3} y2={bottom} />}
      {lanes.map((l, k) => (
        <g key={l.lane}>
          <text className="lane" x="0" y={tops[k] + 26}>{l.name}</text>
          {l.sub && <text className="lane-sub" x="0" y={tops[k] + 42}>{l.sub}</text>}
          {l.pts.map((d) => <circle key={d.id} className={l.cls} cx={x(d.month)} cy={tops[k] + 21} r="5" {...(highlight?.has(d.id) ? { stroke: "var(--accent-strong)", strokeWidth: 3 } : {})} />)}
          {l.placed.map((p) => {
            const lx = p.anchor === "end" ? p.x - 8 : p.x + 8;
            const y = tops[k] + 18 + p.row * ROW;
            return <g key={p.ids.join("+")}>
              {p.row > 0 && <line className="lead" x1={p.x} x2={p.x} y1={tops[k] + 26} y2={y - 8} />}
              <text className="lbl" x={lx} y={y} textAnchor={p.anchor}>{p.label}</text>
              <text x={lx} y={y + 13} textAnchor={p.anchor}>{p.date}</text>
            </g>;
          })}
        </g>
      ))}
    </svg>
  );
}

/** A saved sample date, shown highlighted until the page is left. */
function fromPreview(saved: Saved, p: AddPreview): LifeDate {
  const f = Object.fromEntries(p.fields);
  const m = /([A-Z][a-z]{2}) (\d{1,2}), (\d{4})/.exec(f.Date ?? "");
  const mi = m ? MONTHS.indexOf(m[1]) : 11;
  return { id: saved.id, title: saved.title, short: saved.title, sub: "you typed it · just now", date: m ? `${m[1]} ${m[2]}` : "", month: Math.max(0, mi) + (m ? Number(m[2]) / 31 : 0), reminder: (f.Reminders ?? "").split(" · ")[0], due: false, lane: f.Lane === "Japan · residence" ? "residence" : "personal" };
}

/** The stored row behind a date (the wire fields `life_date_upsert` takes). */
type DateRow = { id: string; title: string; date: string; lane: string; sub: string; leadDays: number[]; goalId: string; done: boolean };
const rowOf = (d: LifeDate): DateRow | null => (d.iso ? { id: d.id, title: d.title, date: d.iso, lane: d.lane, sub: d.rawSub ?? "", leadDays: d.leadDays ?? [], goalId: d.goalId ?? "", done: d.done ?? false } : null);
/** "90, 30, 7" or "2 weeks" style numbers: only whole days, 1 to 730, at most four; blank means the default ladder. */
export function parseLeadDays(text: string): number[] | null {
  const nums = text.split(/[^0-9]+/).filter(Boolean).map(Number);
  if (nums.length > 4 || nums.some((n) => n < 1 || n > 730)) return null;
  return [...new Set(nums)].sort((a, b) => b - a);
}
const leadText = (d: number[]) => d.join(", ");
const LANE_OPTIONS = [{ value: "personal", label: "Personal" }, { value: "residence", label: "Japan · residence" }];
/** Module-level so the Undo window's start is read when an action finishes, never during render. */
const stampNow = () => Date.now();
const saveRow = (r: DateRow) => realInvoke("life_date_upsert", { draft: { id: r.id, title: r.title, date: r.date, lane: r.lane, sub: r.sub || null, leadDays: r.leadDays, goalId: r.goalId, done: r.done } });

/** PLAN-0055: ranges beyond the 12-month view. "12m" is the designed default (the adapter's upcoming list). */
export type RangeKey = "12m" | "24m" | "5y" | "past" | "custom";
const RANGE_OPTIONS: Array<{ value: RangeKey; label: string }> = [
  { value: "12m", label: "Next 12 months" }, { value: "24m", label: "Next 2 years" }, { value: "5y", label: "Next 5 years" },
  { value: "past", label: "Past 12 months" }, { value: "custom", label: "Custom range" },
];
export function rangeOf(key: RangeKey, today: string, custom: { from: string; to: string }): { from: string; to: string } {
  if (key === "24m") return { from: today, to: addDays(today, 730) };
  if (key === "5y") return { from: today, to: addDays(today, 1826) };
  if (key === "past") return { from: addDays(today, -365), to: today };
  if (key === "custom") return custom;
  return { from: today, to: addDays(today, 365) };
}
const ISO = /^\d{4}-\d{2}-\d{2}$/;
const spanDays = (a: string, b: string) => Math.round((Date.parse(`${b}T00:00:00Z`) - Date.parse(`${a}T00:00:00Z`)) / 86_400_000);
const when = (n: number) => (n === 0 ? "today" : n > 0 ? `in ${n} day${n === 1 ? "" : "s"}` : `${-n} day${n === -1 ? "" : "s"} ago`);

/** Life › Dates & deadlines (was Today › Life admin): add, timeline, upcoming. Dates only from Taisun or his documents. */
export function DatesView({ adapter, onOpen }: { adapter: PersonalAdapter; onOpen?: (route: Route) => void }) {
  const tick = useStoresTick();
  const today = jstToday();
  const [rangeKey, setRangeKey] = useState<RangeKey>("12m");
  const [custom, setCustom] = useState({ from: today, to: addDays(today, 730) });
  const [ranged, setRanged] = useState<{ key: string; dates: LifeDate[] } | "failed" | null>(null);
  const range = rangeOf(rangeKey, today, custom);
  const rangeOk = ISO.test(range.from) && ISO.test(range.to) && spanDays(range.from, range.to) >= 0 && spanDays(range.from, range.to) <= 3650;
  const months = Math.max(1, Math.ceil(spanDays(range.from, range.to) / 30.4));
  const rangeId = `${range.from}|${range.to}|${tick}`;
  const la = adapter.lifeAdmin();
  const v = valueOf(la);
  const [added, setAdded] = useState<LifeDate[]>([]);
  const add = addAdapter("date");
  const fresh = new Set(added.map((d) => d.id));
  // Edit and delete work on the stored row (life_date_upsert / life_date_delete). Neither is logged in History, so Undo puts the
  // old row back with the same id (upsert), and only while this page stays open.
  const [editing, setEditing] = useState<DateRow | null>(null);
  const [leads, setLeads] = useState("");
  const goalOptions = [{ value: "", label: "No goal" }, ...(valueOf(adapter.goals())?.milestones ?? []).map((m) => ({ value: m.id, label: m.title }))];
  const goalName = (id?: string | null) => (id ? goalOptions.find((o) => o.value === id)?.label : undefined);
  const [deleting, setDeleting] = useState<LifeDate | null>(null);
  const [done, setDone] = useState<{ text: string; at: number; undo?: () => void; error?: boolean } | null>(null);
  const live = inTauri();
  useEffect(() => {
    if (rangeKey === "12m" || !rangeOk || !live) return;
    let on = true;
    const offset = spanDays(today, range.from);
    realInvoke<LifeDateWire[]>("life_date_range", { from: range.from, to: range.to })
      .then((rows) => { if (on) setRanged({ key: rangeId, dates: (Array.isArray(rows) ? rows : []).map((r) => lifeDateFrom(r, offset)) }); }, () => { if (on) setRanged("failed"); });
    return () => { on = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rangeKey, rangeId, rangeOk, live]);
  const inRange = rangeKey !== "12m";
  const rangedDates = inRange && ranged && ranged !== "failed" ? ranged.dates : null;
  const fail = (text: string) => setDone({ text, at: stampNow(), error: true });
  const submitEdit = async () => {
    if (!editing) return;
    const prev = v?.dates.find((d) => d.id === editing.id); const before = prev ? rowOf(prev) : null;
    if (!editing.title.trim() || !/^\d{4}-\d{2}-\d{2}$/.test(editing.date)) { fail("Give it a title and a full date. Nothing was changed."); return; }
    const leadDays = parseLeadDays(leads);
    if (!leadDays) { fail("Reminders are whole days before the date, up to four of them (for example 90, 14, 3). Nothing was changed."); return; }
    try { await saveRow({ ...editing, title: editing.title.trim(), leadDays }); } catch { fail("That date didn't save. Nothing was changed."); return; }
    setEditing(null); announceAdds();
    setDone({ text: `Saved “${editing.title.trim()}”. Its reminders were recalculated.`, at: stampNow(), undo: before ? () => { void saveRow(before).then(() => { announceAdds(); setDone({ text: `Put “${before.title}” back as it was.`, at: stampNow() }); }, () => fail("That couldn't be undone. Edit the date again to fix it.")); } : undefined });
  };
  // Done stops its reminders and takes it out of Needs you and Amara's upcoming list; Reopen brings it back.
  const markDone = async (d: LifeDate, finished: boolean) => {
    const row = rowOf(d); if (!row) return;
    try { await saveRow({ ...row, done: finished }); } catch { fail("That didn't save. Nothing was changed."); return; }
    announceAdds();
    setDone({ text: finished ? `Marked “${d.title}” done. Its reminders stopped.` : `Reopened “${d.title}”. Its reminders are back.`, at: stampNow(), undo: () => { void saveRow(row).then(() => { announceAdds(); setDone({ text: `Put “${d.title}” back as it was.`, at: stampNow() }); }, () => fail("That couldn't be undone. Try again.")); } });
  };
  const confirmDelete = async () => {
    const d = deleting; setDeleting(null); if (!d) return;
    const row = rowOf(d);
    try { await realInvoke<boolean>("life_date_delete", { id: d.id }); } catch { fail("That date wasn't deleted. Nothing was changed."); return; }
    announceAdds();
    setDone({ text: `Deleted “${d.title}”. Its reminders are gone.`, at: stampNow(), undo: row ? () => { void saveRow(row).then(() => { announceAdds(); setDone({ text: `Put “${row.title}” back.`, at: stampNow() }); }, () => fail("That couldn't be undone. Add the date again.")); } : undefined });
  };
  // A live list already holds the saved date once re-read; the highlighted copy replaces it, never doubles it.
  const everything = rangedDates ? [...rangedDates].sort((a, b) => a.month - b.month) : v ? [...v.dates.filter((d) => !fresh.has(d.id)), ...added].sort((a, b) => a.month - b.month) : added;
  const spanLabel = RANGE_OPTIONS.find((o) => o.value === rangeKey)?.label.toLowerCase() ?? "";
  const all = everything.filter((d) => !d.done);
  const finished = everything.filter((d) => d.done);
  const composer = (
    <Composer title="Add a date" kind="Date" adapter={add} placeholder="e.g. Visa expires Sept 14 next year, remind me 3 months before"
      hint="Amara fills in the date and reminders and shows a preview. Dates come from you or your documents; she never guesses a legal deadline. An Enterprise date goes to Enterprise › Ventures."
      working="Reading your date…" saveLabel="Save date" onOpen={onOpen}
      onSaved={(saved, preview) => setAdded((a) => [...a, fromPreview(saved, preview)])} onUndone={(id) => setAdded((a) => a.filter((d) => d.id !== id))}
      savedText={(s) => `Added “${s.title}” to Dates & deadlines. It shows in Needs you today when its reminder starts.`} />
  );
  return (
    <div className="v-page" id="dates">
      <PageHero title="Dates & deadlines" statement={v ? `Next up: ${v.next}` : "Dates that matter"}
        detail={v ? <>{all.length} dates in {inRange && rangedDates ? spanLabel : "the next 12 months"} · reminders at {PERSONAL_DEFAULTS.lifeAdminLadder.join(", ")} days unless you pick your own · Enterprise dates are read-only here {la.state === "sample" && <DataTag kind="sample" />}</> : <>Visa, residence card and contract dates come from you or your documents <DataTag kind="wire" /></>} />
      <AmaraTake page="life" className="life__take" />
      <p className="pk-note">For things with a hard date that need lead time: visas, renewals, filings, contracts. Soft targets belong in Goals and small to-dos in Lists. Amara reminds you at the lead times you pick (or 90, 30 and 7 days before), shows what is close in Needs you, and knows these dates when you chat. Mark a date done when it is handled.</p>
      {composer}
      {done && <ResultLine error={done.error} onUndo={done.undo} startedAt={done.at}>{done.text}</ResultLine>}
      {!v ? <Card title="Next 12 months" accessory={<span className="pk-acc"><DataTag kind="wire" /></span>}><p className="pk-note">Needs wiring: dates appear once the life dates store exists (HANDOFF-1111a). Nothing here is made up.</p></Card> : (
        <>
          <Card title="Timeline" accessory={<span className="pk-acc">{la.state === "sample" && <DataTag kind="sample" />}<span className="v-note">{inRange ? `${range.from} – ${range.to}` : "Next 12 months"}</span></span>}>
            <form className="lf-range" aria-label="Date range" onSubmit={(e) => e.preventDefault()}>
              <Select label="Show" options={RANGE_OPTIONS} value={rangeKey} onChange={(e) => { setRangeKey(e.target.value as RangeKey); setRanged(null); }} disabled={!live && la.state !== "sample"} />
              {rangeKey === "custom" && <>
                <TextField label="From" type="date" value={custom.from} onChange={(e) => setCustom((c) => ({ ...c, from: e.target.value }))} />
                <TextField label="To" type="date" value={custom.to} onChange={(e) => setCustom((c) => ({ ...c, to: e.target.value }))} />
              </>}
            </form>
            {inRange && !live ? <p className="pk-note">Ranges read the dates store on this Mac; the preview shows the next 12 months only.</p>
              : inRange && !rangeOk ? <p className="pk-note lf-err" role="alert">Pick a start before the end, up to 10 years apart.</p>
              : inRange && ranged === "failed" ? <p className="pk-note lf-err">Dates in that range couldn&apos;t be read just now.</p>
              : inRange && !rangedDates ? <p className="pk-note">Reading dates…</p>
              : inRange ? <LaneTimeline dates={everything} months={months} ticks={rangeTicks(range.from, months)} span={spanLabel} />
              : <LaneTimeline dates={all} highlight={fresh} />}
          </Card>
          <Card title={inRange && rangedDates ? `In ${spanLabel}` : "Upcoming"} accessory={<span className="pk-acc">{la.state === "sample" && <DataTag kind="sample" />}</span>}>
            <table className="pk-table" aria-label={inRange && rangedDates ? `Dates in ${spanLabel}` : "Upcoming"}>
              <thead><tr><th scope="col">What</th><th scope="col">Lane</th><th scope="col">Date</th><th scope="col">Reminder</th><th scope="col"><span className="ui-visually-hidden">Action</span></th></tr></thead>
              <tbody>{all.map((d) => (
                <tr key={d.id} className={fresh.has(d.id) ? "v-new" : undefined}>
                  <td>{d.title}<small>{d.sub}{goalName(d.goalId) ? ` · Goal: ${goalName(d.goalId)}` : ""}</small></td>
                  <td>{LANE_NAME[d.lane]}</td>
                  <td>{d.date}{d.inDays != null && <small>{when(d.inDays)}</small>}</td>
                  <td><StatusText kind={d.due ? "wait" : "off"}>{d.reminder}</StatusText></td>
                  <td className="r">{d.lane === "enterprise"
                    ? <><span className="v-ro">Read-only · </span><LinkButton small onClick={onOpen ? () => onOpen(enterpriseLink(d)) : undefined} label={`Edit in Enterprise: ${d.title}`}>Edit in Enterprise ›</LinkButton></>
                    : <>
                      <Button size="small" variant="plain" disabled={!live || !d.iso || fresh.has(d.id)} aria-label={`Done: ${d.title}`} onClick={() => { void markDone(d, true); }}>Done</Button>
                      <Button size="small" variant="plain" disabled={!live || !d.iso || fresh.has(d.id)} aria-label={`Edit: ${d.title}`} onClick={() => { const r = rowOf(d); if (r) { setEditing(r); setLeads(leadText(r.leadDays)); } }}>Edit</Button>
                      <Button size="small" variant="plain" disabled={!live || fresh.has(d.id)} aria-label={`Delete: ${d.title}`} onClick={() => setDeleting(d)}>Delete</Button>
                    </>}</td>
                </tr>
              ))}</tbody>
            </table>
          </Card>
          {finished.length > 0 && (
            <Card title="Done">
              <table className="pk-table" aria-label="Done dates">
                <tbody>{finished.map((d) => (
                  <tr key={d.id}><td>{d.title}<small>{d.sub}</small></td><td>{d.date}</td><td className="r"><Button size="small" variant="plain" disabled={!live} aria-label={`Reopen: ${d.title}`} onClick={() => { void markDone(d, false); }}>Reopen</Button></td></tr>
                ))}</tbody>
              </table>
            </Card>
          )}
          {editing && (
            <Card title={`Edit: ${editing.title || "date"}`}>
              <form className="ci-form" onSubmit={(e) => { e.preventDefault(); void submitEdit(); }}>
                <div className="pk-g2">
                  <TextField label="What" value={editing.title} maxLength={140} onChange={(e) => setEditing({ ...editing, title: e.target.value })} />
                  <TextField label="Date" type="date" value={editing.date} onChange={(e) => setEditing({ ...editing, date: e.target.value })} />
                </div>
                <div className="pk-g2">
                  <Select label="Lane" options={LANE_OPTIONS} value={editing.lane} onChange={(e) => setEditing({ ...editing, lane: e.target.value })} />
                  <TextField label="Note" value={editing.sub} maxLength={140} onChange={(e) => setEditing({ ...editing, sub: e.target.value })} />
                </div>
                <div className="pk-g2">
                  <TextField label="Remind me (days before)" value={leads} placeholder="blank = 90, 30, 7 (up to four)" onChange={(e) => setLeads(e.target.value)} />
                  <Select label="Goal" options={goalOptions} value={editing.goalId} onChange={(e) => setEditing({ ...editing, goalId: e.target.value })} />
                </div>
                <div className="pk-actions"><Button size="small" variant="primary" type="submit">Save date</Button><Button size="small" variant="plain" onClick={() => setEditing(null)}>Cancel</Button></div>
              </form>
            </Card>
          )}
          <Dialog open={deleting !== null} title={deleting ? `Delete “${deleting.title}”?` : "Delete this date?"} destructive confirmLabel="Delete" onConfirm={() => { void confirmDelete(); }} onCancel={() => setDeleting(null)}>
            Its reminders stop. You can undo this until you leave the page.
          </Dialog>
        </>
      )}
    </div>
  );
}
