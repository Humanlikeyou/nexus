import { useState } from "react";
import { formatMinutes } from "../lib/format";
import { Card, Figure, Figures, Meter, SegmentedControl } from "../ui";
import { loadWorkouts, useLoad } from "./noopData";
import { PanelState, SampleTag } from "./PanelState";
import { ZONE_NAMES, workoutRows, workoutSummary, type WorkoutRow } from "./workoutsModel";

const RANGE = [{ value: "7", label: "Week" }, { value: "14", label: "2 weeks" }, { value: "30", label: "Month" }, { value: "90", label: "3 months" }] as const;
type Range = (typeof RANGE)[number]["value"];
const zoneColor = (i: number) => `var(--fx-zone-${i + 1})`;

function ZoneBar({ row }: { row: WorkoutRow }) {
  if (!row.zones) return <p className="fx-muted">No zone data for this session.</p>;
  return (
    <div className="fx-zones">
      <Meter size="sm" label={`${row.sport} heart-rate zones`} segments={row.zones.map((v, i) => ({ label: ZONE_NAMES[i], value: v, color: zoneColor(i) }))} format={formatMinutes} />
      <ul className="fx-legend" aria-hidden="true">
        {row.zones.map((m, i) => m > 0 && <li key={i}><i style={{ background: zoneColor(i) }} />{ZONE_NAMES[i]} <span className="fx-num">{formatMinutes(m)}</span></li>)}
      </ul>
    </div>
  );
}

/** Fitness › Workouts (0017 #1): sessions from noop_recent_workouts with minutes in each heart-rate zone. */
export function WorkoutsPanel() {
  const [range, setRange] = useState<Range>("14");
  const [load, retry] = useLoad(() => loadWorkouts(Number(range)), `workouts-${range}`);
  const rows = load.state === "live" || load.state === "sample" ? workoutRows(load.value) : [];
  const sum = workoutSummary(rows);
  const rangeText = RANGE.find((r) => r.value === range)?.label.toLowerCase() ?? "";
  return (
    <div className="fx" data-tab="workouts">
      <Card title="Workouts" accessory={<>{load.state === "sample" && <SampleTag />}<SegmentedControl label="Workout range" size="small" value={range} options={RANGE} onChange={setRange} /></>}>
        {rows.length === 0 && <PanelState load={load} what="workouts" emptyHint={`No sessions in the last ${rangeText === "week" ? "week" : rangeText}. Workouts your strap detects show up after a sync.`} onRetry={retry} />}
        {rows.length > 0 && (
          <>
            <Figures className="fx-figures">
              <Figure label="Sessions" value={sum.sessions} />
              <Figure label="Active time" value={formatMinutes(sum.minutes)} />
              <Figure label="Time in zones 4–5" value={sum.withZones ? formatMinutes(sum.zoneTotals[3] + sum.zoneTotals[4]) : "No zone data"} />
            </Figures>
            {sum.withZones > 0 && (
              <div className="fx-block">
                <h3 className="fx-h">Time in each zone</h3>
                <Meter label={`Minutes in each heart-rate zone across ${sum.withZones} ${sum.withZones === 1 ? "session" : "sessions"}`} segments={sum.zoneTotals.map((v, i) => ({ label: ZONE_NAMES[i], value: v, color: zoneColor(i) }))} format={formatMinutes} />
                <ul className="fx-legend">
                  {sum.zoneTotals.map((m, i) => <li key={i}><i style={{ background: zoneColor(i) }} aria-hidden="true" />{ZONE_NAMES[i]} <span className="fx-num">{formatMinutes(m)}</span></li>)}
                </ul>
                {sum.withZones < sum.sessions && <p className="fx-muted">{sum.sessions - sum.withZones} {sum.sessions - sum.withZones === 1 ? "session has" : "sessions have"} no zone data and {sum.sessions - sum.withZones === 1 ? "is" : "are"} left out of this bar.</p>}
              </div>
            )}
            <ul className="fx-list" aria-label="Workouts">
              {rows.map((r) => (
                <li key={r.key} className="fx-item">
                  <div className="fx-item__head">
                    <h3 className="fx-item__title">{r.sport}</h3>
                    <span className="fx-item__when">{r.dayLabel}{r.time ? ` · ${r.time}` : ""}</span>
                  </div>
                  {r.facts && <p className="fx-item__facts">{r.facts}</p>}
                  {(r.steps != null || r.strain != null) && <p className="fx-item__facts">{[r.strain != null ? `Effort ${Math.round(r.strain)}` : null, r.steps != null ? `${r.steps.toLocaleString("en-US")} steps` : null].filter(Boolean).join(" · ")}</p>}
                  <ZoneBar row={r} />
                </li>
              ))}
            </ul>
          </>
        )}
      </Card>
    </div>
  );
}
