import { useState } from "react";
import type { Route } from "../app/routes";
import { addAdapter } from "../adds/adapters";
import { Composer, LinkButton, OffButton } from "../adds/Composer";
import type { AddPreview, Saved } from "../adds/flow";
import { valueOf } from "../lib/sampleAdapters";
import { PERSONAL_DEFAULTS, type LifeDate, type LifeLane, type PersonalAdapter } from "../personal/adapters";
import { AmaraTake } from "../amaraTake/AmaraTakeCard";
import { Card, DataTag, PageHero, StatusText } from "../ui";
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
const x = (m: number) => L + (m / 12) * (W - L - 13);

/** Where an Enterprise date is edited (it is read-only here). */
const enterpriseLink = (d: LifeDate): Route => (/taiyo skin launch/i.test(d.title) ? { id: "enterprise", view: "ventures/taiyo-skin" } : /tax/i.test(d.title) ? { id: "enterprise", view: "finances" } : { id: "enterprise", view: "ventures" });

/** design-1111v LaneTimeline: 12 months, three lanes, labels that never overlap (placeLabels). */
export function LaneTimeline({ dates, highlight }: { dates: LifeDate[]; highlight?: Set<string> }) {
  const lanes = LANES.map((l) => {
    const pts = dates.filter((d) => d.lane === l.lane);
    const placed = placeLabels(pts.map((d) => ({ id: d.id, x: x(d.month), label: d.short ?? d.title, date: d.date })), { width: W, rows: 2 });
    const rows = Math.max(1, ...placed.map((p) => p.row + 1));
    return { ...l, pts, placed, h: Math.max(68, 21 + rows * ROW + 13) };
  });
  const tops = lanes.reduce<number[]>((acc, l, i) => [...acc, i === 0 ? TOP : acc[i - 1] + lanes[i - 1].h], []);
  const bottom = tops[tops.length - 1] + lanes[lanes.length - 1].h;
  return (
    <svg className="v-tl" viewBox={`0 0 ${W} ${bottom + 8}`} role="img" aria-label={`Dates over the next 12 months in three lanes: ${dates.map((d) => `${d.title}, ${d.date}, ${LANE_NAME[d.lane]}`).join("; ")}`}>
      {MONTHS.map((m, i) => <g key={m}><line className="grid" x1={x(i)} x2={x(i)} y1={TOP - 8} y2={bottom} /><text x={x(i) + 5} y={TOP - 8}>{m}</text></g>)}
      <line className="today" x1={x(0)} x2={x(0)} y1={TOP - 3} y2={bottom} />
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

/** Life › Dates & deadlines (was Today › Life admin): add, timeline, upcoming. Dates only from Taisun or his documents. */
export function DatesView({ adapter, onOpen }: { adapter: PersonalAdapter; onOpen?: (route: Route) => void }) {
  const la = adapter.lifeAdmin();
  const v = valueOf(la);
  const [added, setAdded] = useState<LifeDate[]>([]);
  const add = addAdapter("date");
  const all = v ? [...v.dates, ...added].sort((a, b) => a.month - b.month) : added;
  const fresh = new Set(added.map((d) => d.id));
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
        detail={v ? <>{v.dates.length} dates in the next 12 months · reminders at {PERSONAL_DEFAULTS.lifeAdminLadder.join(", ")} days · Enterprise dates are read-only here <DataTag kind="sample" /></> : <>Visa, residence card and contract dates come from you or your documents <DataTag kind="wire" /></>} />
      <AmaraTake page="life" className="life__take" />
      {composer}
      {!v ? <Card title="Next 12 months" accessory={<span className="pk-acc"><DataTag kind="wire" /></span>}><p className="pk-note">Needs wiring: dates appear once the life dates store exists (HANDOFF-1111a). Nothing here is made up.</p></Card> : (
        <>
          <Card title="Timeline" accessory={<span className="pk-acc"><DataTag kind="sample" /><span className="v-note">Next 12 months</span></span>}>
            <LaneTimeline dates={all} highlight={fresh} />
          </Card>
          <Card title="Upcoming" accessory={<span className="pk-acc"><DataTag kind="sample" /></span>}>
            <table className="pk-table" aria-label="Upcoming">
              <thead><tr><th scope="col">What</th><th scope="col">Lane</th><th scope="col">Date</th><th scope="col">Reminder</th><th scope="col"><span className="ui-visually-hidden">Action</span></th></tr></thead>
              <tbody>{all.map((d) => (
                <tr key={d.id} className={fresh.has(d.id) ? "v-new" : undefined}>
                  <td>{d.title}<small>{d.sub}</small></td>
                  <td>{LANE_NAME[d.lane]}</td>
                  <td>{d.date}{d.inDays != null && <small>in {d.inDays} days</small>}</td>
                  <td><StatusText kind={d.due ? "wait" : "off"}>{d.reminder}</StatusText></td>
                  <td className="r">{d.lane === "enterprise"
                    ? <><span className="v-ro">Read-only · </span><LinkButton small onClick={onOpen ? () => onOpen(enterpriseLink(d)) : undefined} label={`Edit in Enterprise: ${d.title}`}>Edit in Enterprise ›</LinkButton></>
                    : <OffButton variant="plain" reason="Needs wiring · life_date_upsert" label={`Edit: ${d.title}`}>Edit</OffButton>}</td>
                </tr>
              ))}</tbody>
            </table>
          </Card>
        </>
      )}
    </div>
  );
}
