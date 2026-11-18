import { Check, Flame, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { roadmapAdapter, type Roadmap, type RoadmapAdapter, type StepSize } from "../goals/roadmap";
import { useStoresTick } from "../lib/localStores";
import type { Habit } from "../personal/adapters";
import { Dots } from "../personal/parts";
import { reducedMotion } from "../shell/Splash";
import { Button, DataTag, Select, TextField } from "../ui";
import { branchAdapter, type BranchAdapter, type BranchSuggestion } from "./branches";
import {
  SIZE_LABEL, STATION_RADIUS, currentStation, fillFraction, glanceOf, pathLabel, pathPhase, smoothPath, stationPoints, stationsOf, strandPath,
  type Glance, type PathPhase, type Station,
} from "./pathModel";
import "./design1118.css";

/** One goal on the map: its title, tag and the roadmap that draws its path. */
export type PathGoal = { id: string; title: string; tag?: string; detail?: string };
type GoalState = { goal: PathGoal; rm: Roadmap | null; phase: PathPhase; sample: boolean; stations: Station[]; glance: Glance };

const SIZE_OPTIONS = (["small", "medium", "big"] as const).map((v) => ({ value: v, label: SIZE_LABEL[v] }));
const NOT_SAVED = "Not saved. This needs the roadmap store to be wired first.";

/** The percent ring with steps under it: one progress meaning ("11% · 1 of 9"). */
export function GlanceRing({ glance, size = 52 }: { glance: Glance; size?: number }) {
  const r = size / 2 - 5, c = 2 * Math.PI * r;
  const frac = glance.pct != null ? glance.pct / 100 : glance.total ? glance.done / glance.total : 0;
  return (
    <span className="d18-ring" style={{ width: size, height: size }} aria-hidden="true">
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle className="d18-ring__track" cx={size / 2} cy={size / 2} r={r} />
        <circle className="d18-ring__fill" cx={size / 2} cy={size / 2} r={r} strokeDasharray={`${(frac * c).toFixed(2)} ${c.toFixed(2)}`} transform={`rotate(-90 ${size / 2} ${size / 2})`} />
      </svg>
      <span className="d18-ring__v"><b>{glance.short}</b>{glance.total > 0 && glance.pct != null && <small>{glance.done} of {glance.total}</small>}{glance.total > 0 && glance.pct == null && <small>steps</small>}</span>
    </span>
  );
}

type BranchDraw = { from: number; rejoin: number; steps: BranchSuggestion["alternateSteps"]; state: "proposed" | "accepted" | "retracting" };

/** The path itself. Planning draws only a seed (no stroke). `grow` draws the stroke in, station by station. */
export function PathSvg({ stations, phase, seed = 0, drill = false, grow = false, branch, celebrate, label }: {
  stations: Station[]; phase: PathPhase; seed?: number; drill?: boolean; grow?: boolean; branch?: BranchDraw | null;
  celebrate?: { stationId: string; big: boolean; phase: string } | null; label: string;
}) {
  const w = drill ? 640 : 320, h = drill ? 170 : 96;
  if (phase !== "ready") {
    return (
      <svg className="d18-path" data-phase={phase} viewBox={`0 0 ${w} ${h}`} role="img" aria-label={label}>
        <g className="d18-seed" transform={`translate(${w / 2} ${h / 2})`}><circle r={drill ? 10 : 7} /><circle className="d18-seed__halo" r={drill ? 18 : 12} /></g>
      </svg>
    );
  }
  const live = stations.filter((s) => s.state !== "ghost");
  const pts = stationPoints(live.length, w, h, seed, drill ? 28 : 16);
  const d = smoothPath(pts);
  const fill = fillFraction(live);
  const here = live.findIndex((s) => s.state === "current");
  let branchD = "", ghostD = "", branchPts: { x: number; y: number }[] = [];
  if (branch && pts[branch.from] && pts[branch.rejoin]) {
    const a = pts[branch.from], b = pts[branch.rejoin];
    const dip = drill ? 46 : 26;
    branchPts = branch.steps.map((_, i) => { const t = (i + 1) / (branch.steps.length + 1); return { x: a.x + (b.x - a.x) * t, y: Math.min(h - 8, Math.max(a.y, b.y) + dip * Math.sin(Math.PI * t)) }; });
    branchD = smoothPath([a, ...branchPts, b]);
    if (branch.state === "accepted") ghostD = smoothPath(pts.slice(branch.from, branch.rejoin + 1));
  }
  return (
    <svg className="d18-path" data-phase="ready" data-drill={drill || undefined} data-grow={grow || undefined} viewBox={`0 0 ${w} ${h}`} role="img" aria-label={label}>
      {ghostD ? (
        <>
          <path className="d18-path__track" d={smoothPath(pts.slice(0, branch!.from + 1))} pathLength={1} />
          <path className="d18-path__ghost" d={ghostD} />
          <path className="d18-path__track" d={smoothPath(pts.slice(branch!.rejoin))} pathLength={1} />
        </>
      ) : <path className="d18-path__track" d={d} pathLength={1} />}
      {!ghostD && <path className="d18-path__fill" d={d} pathLength={1} style={{ "--fill": fill } as CSSProperties} data-cele={celebrate?.phase} />}
      {branchD && <path className={`d18-path__branch d18-path__branch--${branch!.state}`} d={branchD} pathLength={1} />}
      {branchPts.map((p, i) => <circle key={`b${i}`} className="d18-st d18-st--branch" cx={p.x} cy={p.y} r={STATION_RADIUS[branch!.steps[i].size] * (drill ? 1 : 0.62)} />)}
      {live.map((s, i) => {
        const p = pts[i]; const r = STATION_RADIUS[s.size] * (drill ? 1 : 0.62);
        const ghost = ghostD && i > branch!.from && i < branch!.rejoin;
        const cele = celebrate?.stationId === s.id ? celebrate : null;
        return (
          <g key={s.id} className={`d18-st d18-st--${ghost ? "ghost" : s.state}`} style={{ "--i": i, "--n": live.length } as CSSProperties} data-cele={cele?.phase}>
            {s.state === "current" && !ghost && <circle className="d18-st__halo" cx={p.x} cy={p.y} r={r + (drill ? 7 : 4)} />}
            {cele && cele.big && <g className="d18-burst" transform={`translate(${p.x} ${p.y})`}>{Array.from({ length: 10 }, (_, k) => <line key={k} x1="0" y1={-(r + 6)} x2="0" y2={-(r + 18)} transform={`rotate(${k * 36})`} />)}<circle r={r + 10} /></g>}
            <circle className="d18-st__dot" cx={p.x} cy={p.y} r={r} />
            {s.state === "done" && !ghost && <path className="d18-st__check" d={`M${p.x - r * 0.45},${p.y} l${r * 0.32},${r * 0.34} l${r * 0.6},-${r * 0.66}`} />}
            {drill && s.title && <text className="d18-st__label" x={p.x} y={p.y + (i % 2 ? r + 16 : -(r + 9))} textAnchor="middle">{s.title.length > 26 ? `${s.title.slice(0, 24)}…` : s.title}</text>}
            {drill && s.state === "current" && !ghost && <text className="d18-st__here" x={p.x} y={p.y - (i % 2 ? r + 9 : r + 26)} textAnchor="middle">YOU ARE HERE</text>}
          </g>
        );
      })}
      {celebrate?.phase === "travel" && here >= 0 && <circle className="d18-traveler" cx={pts[here].x} cy={pts[here].y} r={drill ? 5 : 3} />}
    </svg>
  );
}

function useGoalStates(goals: PathGoal[], adapter: RoadmapAdapter, over: Record<string, Set<string>>): GoalState[] {
  const tick = useStoresTick();
  return useMemo(() => {
    void tick;
    return goals.map((goal) => {
      const w = adapter.roadmap(goal.id);
      const rm = w.state === "unwired" ? null : w.value;
      const phase = pathPhase(rm, w.state === "unwired");
      const done = over[goal.id];
      const steps = (rm?.steps ?? []).map((s) => (done?.has(s.id) ? { ...s, done: true } : s));
      const stations = phase === "ready" ? stationsOf(steps) : [];
      const glance = glanceOf(rm, stations, !!done && done.size > 0);
      return { goal, rm, phase, sample: w.state === "sample", stations, glance };
    });
  }, [goals, adapter, over, tick]);
}

function PathCard({ g, seed, onOpen }: { g: GoalState; seed: number; onOpen: () => void }) {
  const here = currentStation(g.stations);
  const foot = g.phase === "planning" ? "Amara is planning the route" : g.phase === "unwired" ? "Route appears once the roadmap store answers" : here ? `You are here · ${here.title ?? "step"} · ${SIZE_LABEL[here.size]}` : "All steps done";
  return (
    <button type="button" className="d18-pcard" onClick={onOpen} aria-label={`Open ${g.goal.title}. ${pathLabel(g.goal.title, g.glance, g.stations)}`}>
      <span className="d18-pcard__head"><span className="d18-pcard__t">{g.goal.title}</span>{g.goal.tag && <span className="d18-chip">{g.goal.tag}</span>}<span className="d18-grow" /><GlanceRing glance={g.glance} /></span>
      <PathSvg stations={g.stations} phase={g.phase} seed={seed} label={pathLabel(g.goal.title, g.glance, g.stations)} />
      <span className="d18-pcard__foot">{foot}</span>
    </button>
  );
}

/** Milestones arrival: the four goals as moving, tangled strands. Focus or hover one to untangle it. */
function Tangle({ goals, onFocus, onGrid }: { goals: GoalState[]; onFocus: (id: string) => void; onGrid: () => void }) {
  return (
    <div className="d18-tangle" data-testid="milestones-tangle">
      <svg viewBox="0 0 680 140" className="d18-tangle__svg" role="img" aria-label={`${goals.length} goals, at rest. Choose one to see its path.`}>
        {goals.map((g, i) => (
          <g key={g.goal.id} className="d18-strand" data-k={i % 4}>
            <path d={strandPath(i, 1360, 140)} className="d18-strand__line" />
          </g>
        ))}
      </svg>
      <div className="d18-legend">
        {goals.map((g, i) => <button key={g.goal.id} type="button" className="d18-legend__b" data-k={i % 4} onMouseEnter={() => undefined} onClick={() => onFocus(g.goal.id)}><i aria-hidden="true" />{g.goal.title}</button>)}
        <span className="d18-grow" />
        <Button size="small" variant="plain" onClick={onGrid}>See all paths</Button>
      </div>
      <p className="d18-note">Focus a strand to untangle its path.</p>
    </div>
  );
}

type Celebration = { goalId: string; stationId: string; big: boolean; phase: "charge" | "burst" | "travel" | "settle" | "reduced"; line: string };

function NextActionCard({ g, onYes, onNotYet, onEdit, onAccept, msg }: { g: GoalState; onYes: () => void; onNotYet: () => void; onEdit: (title: string, size: StepSize) => void; onAccept: () => void; msg: string }) {
  const here = currentStation(g.stations);
  const step = here ? g.rm?.steps.find((s) => s.id === here.id) : null;
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState({ title: "", size: "small" as StepSize });
  if (g.phase !== "ready" || !here || !step) return null;
  return (
    <section className="d18-next" aria-label="Next step">
      <p className="d18-eyebrow">Next step · {g.goal.title} · {SIZE_LABEL[here.size]} {g.sample && <DataTag kind="sample" />}</p>
      {editing ? (
        <form className="d18-next__edit" onSubmit={(e) => { e.preventDefault(); const t = draft.title.trim(); if (!t) return; onEdit(t, draft.size); setEditing(false); }}>
          <TextField label="Step" value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} />
          <Select label="Size" options={SIZE_OPTIONS} value={draft.size} onChange={(e) => setDraft({ ...draft, size: e.target.value as StepSize })} />
          <Button size="small" variant="primary" type="submit">Save step</Button>
          <Button size="small" variant="plain" onClick={() => setEditing(false)}>Cancel</Button>
        </form>
      ) : (
        <>
          <h3 className="d18-next__t">{step.title}</h3>
          {step.why && <p className="d18-next__why">{step.why}</p>}
          {g.rm?.status === "suggested" && <p className="d18-next__why">Amara suggested this route. Accept it, or edit any step first.</p>}
          <div className="d18-actions">
            <Button size="small" variant="primary" onClick={onYes}>Yes, it&apos;s done</Button>
            <Button size="small" variant="secondary" onClick={onNotYet}>Not yet</Button>
            <Button size="small" variant="plain" onClick={() => { setDraft({ title: step.title, size: step.size }); setEditing(true); }}>Edit</Button>
            {g.rm?.status === "suggested" && <Button size="small" variant="plain" onClick={onAccept}>Accept route</Button>}
          </div>
        </>
      )}
      <p className="d18-msg" role="status" aria-live="polite">{msg}</p>
    </section>
  );
}

function BranchReasonCard({ b, onAccept, onDecline }: { b: BranchSuggestion; onAccept: () => void; onDecline: () => void }) {
  return (
    <section className="d18-branch" aria-label="Amara suggests another route" data-testid="branch-card">
      <p className="d18-eyebrow">Another route · Amara {b.sample && <DataTag kind="sample" />}</p>
      <p className="d18-branch__why">{b.reasonText}</p>
      <ol className="d18-branch__steps">{b.alternateSteps.map((s) => <li key={s.id}>{s.title} <span className="d18-chip">{SIZE_LABEL[s.size]}</span></li>)}</ol>
      <div className="d18-actions">
        <Button size="small" variant="primary" icon={Check} onClick={onAccept}>Take this route</Button>
        <Button size="small" variant="secondary" icon={X} onClick={onDecline}>Not now</Button>
      </div>
      <p className="d18-note">Nothing about the goal changes until you choose.</p>
    </section>
  );
}

/**
 * Life › Goals, Milestones (design-1118a rounds 2–5): arrival tangle → a goal in focus (drill-in) →
 * the four-path grid, which stays until he leaves the page (no re-tangle on this visit).
 */
export function MilestonesMap({ goals, adapter, branches, sampleTag }: { goals: PathGoal[]; adapter?: RoadmapAdapter; branches?: BranchAdapter; sampleTag?: boolean }) {
  const a = adapter ?? roadmapAdapter();
  const br = branches ?? branchAdapter();
  const [view, setView] = useState<"tangle" | "focus" | "grid">("tangle");
  const [focus, setFocus] = useState<string | null>(null);
  const [over, setOver] = useState<Record<string, Set<string>>>({});
  const [msg, setMsg] = useState("");
  const [cele, setCele] = useState<Celebration | null>(null);
  const [branchState, setBranchState] = useState<Record<string, "accepted" | "retracting" | "declined">>({});
  const [undo, setUndo] = useState<{ id: string; goalId: string } | null>(null);
  const [, bump] = useState(0);
  const states = useGoalStates(goals, a, over);
  const prevPhase = useRef<Record<string, PathPhase>>({});
  const [growing, setGrowing] = useState<Record<string, boolean>>({});
  useEffect(() => {
    // Grow-in only after Amara has decided the steps: a goal seen planning that turns ready draws itself in.
    const next: Record<string, boolean> = {};
    for (const g of states) { if (prevPhase.current[g.goal.id] === "planning" && g.phase === "ready" && !reducedMotion()) next[g.goal.id] = true; prevPhase.current[g.goal.id] = g.phase; }
    if (Object.keys(next).length) { const t0 = window.setTimeout(() => setGrowing((x) => ({ ...x, ...next })), 0); const t = window.setTimeout(() => setGrowing({}), 2200); return () => { window.clearTimeout(t0); window.clearTimeout(t); }; }
  }, [states]);
  useEffect(() => { if (!undo) return; const t = window.setTimeout(() => setUndo(null), 8000); return () => window.clearTimeout(t); }, [undo]);

  const focused = states.find((g) => g.goal.id === focus) ?? null;
  const open = (id: string) => { setFocus(id); setView("focus"); setMsg(""); };
  const leave = () => { setFocus(null); setView("grid"); setCele(null); };

  const branchW = focused ? br.open(focused.goal.id) : null;
  const branch = branchW && branchW.state !== "unwired" ? branchW.value : null;
  const bstate = focused ? branchState[focused.goal.id] : undefined;
  // keep the last suggestion so an accepted / retracting branch can still draw its steps
  const [lastBranch, setLastBranch] = useState<BranchSuggestion | null>(null);
  if (branch && branch.id !== lastBranch?.id) setLastBranch(branch); // by id: open() returns a fresh object each render
  const hereIdx = focused ? focused.stations.findIndex((s) => s.state === "current") : -1;
  const branchDraw: BranchDraw | null = focused && hereIdx >= 0 && (branch || bstate === "accepted" || bstate === "retracting")
    ? { from: hereIdx, rejoin: Math.min(focused.stations.length - 1, hereIdx + 2), steps: branch?.alternateSteps ?? lastBranch?.alternateSteps ?? [], state: bstate === "accepted" ? "accepted" : bstate === "retracting" ? "retracting" : "proposed" }
    : null;

  const yes = async (g: GoalState) => {
    const here = currentStation(g.stations); if (!here) return;
    const idx = g.stations.indexOf(here); const nextSt = g.stations[idx + 1];
    const line = nextSt ? `Done. Next: ${nextSt.title ?? "the next step"}.` : "Done. That was the last step on this route.";
    try {
      const r = await a.answerStep(g.goal.id, here.id, "yes");
      if (r.state === "unwired") { setMsg(NOT_SAVED); return; }
    } catch { setMsg("That didn't save. Try again in a moment."); return; }
    const big = here.size === "big";
    const apply = () => setOver((o) => ({ ...o, [g.goal.id]: new Set([...(o[g.goal.id] ?? []), here.id]) }));
    if (reducedMotion()) { apply(); setCele({ goalId: g.goal.id, stationId: here.id, big, phase: "reduced", line }); window.setTimeout(() => setCele((c) => (c?.phase === "reduced" ? { ...c, phase: "settle" } : c)), 200); return; }
    const seq: Array<[Celebration["phase"], number]> = big ? [["charge", 0], ["burst", 400], ["travel", 900], ["settle", 1500]] : [["charge", 0], ["travel", 320], ["settle", 900]];
    for (const [phase, at] of seq) window.setTimeout(() => { if (phase === "travel") apply(); setCele({ goalId: g.goal.id, stationId: here.id, big, phase, line }); }, at);
  };
  const notYet = async (g: GoalState) => {
    const here = currentStation(g.stations); if (!here) return;
    try { const r = await a.answerStep(g.goal.id, here.id, "not_yet"); setMsg(r.state === "unwired" ? NOT_SAVED : "No problem. Amara will ask again in a few days."); } catch { setMsg("That didn't save. Try again in a moment."); }
  };
  const edit = async (g: GoalState, title: string, size: StepSize) => {
    const here = currentStation(g.stations); if (!here) return;
    try { const r = await a.editStep(g.goal.id, here.id, { title, size }); setMsg(r.state === "unwired" ? NOT_SAVED : "Step updated."); bump((n) => n + 1); } catch { setMsg("That didn't save. Try again in a moment."); }
  };
  const acceptRoute = async (g: GoalState) => { const r = await a.accept(g.goal.id); setMsg(r.state === "unwired" ? NOT_SAVED : "Route accepted."); bump((n) => n + 1); };
  const takeBranch = async (g: GoalState, b: BranchSuggestion) => {
    const r = await br.accept(b.id);
    if (r.state === "unwired") { setMsg("Not saved. This needs the branch store to be wired first."); return; }
    setBranchState((s) => ({ ...s, [g.goal.id]: "accepted" })); setUndo({ id: b.id, goalId: g.goal.id }); setMsg("Route changed. The old route stays as a faint ghost.");
  };
  const declineBranch = async (g: GoalState, b: BranchSuggestion) => {
    const r = await br.decline(b.id);
    if (r.state === "unwired") { setMsg("Not saved. This needs the branch store to be wired first."); return; }
    setBranchState((s) => ({ ...s, [g.goal.id]: "retracting" })); window.setTimeout(() => setBranchState((s) => ({ ...s, [g.goal.id]: "declined" })), reducedMotion() ? 0 : 500); setMsg("Kept the current route.");
  };
  const undoBranch = async () => { if (!undo) return; await br.undo(undo.id); setBranchState((s) => { const n = { ...s }; delete n[undo.goalId]; return n; }); setUndo(null); setMsg("Back to the previous route."); };

  const seedOf = (id: string) => Math.max(0, goals.findIndex((g) => g.id === id));
  return (
    <section className="d18-map" id="milestones" aria-label="Milestones" data-view={view}>
      <header className="d18-map__head"><h2 className="d18-h2">Milestones</h2>{sampleTag && <DataTag kind="sample" />}<span className="d18-grow" />
        {view === "focus" && <Button size="small" variant="plain" onClick={leave}>All goals</Button>}
      </header>
      {states.length === 0 && <p className="d18-note">Goals you set with Amara appear here as paths.</p>}
      {states.length > 0 && view === "tangle" && <Tangle goals={states} onFocus={open} onGrid={() => setView("grid")} />}
      {states.length > 0 && view === "grid" && (
        <div className="d18-grid" data-testid="milestones-grid">
          {states.map((g) => <PathCard key={g.goal.id} g={{ ...g }} seed={seedOf(g.goal.id)} onOpen={() => open(g.goal.id)} />)}
        </div>
      )}
      {focused && view === "focus" && (
        <div className="d18-drill" data-testid="milestones-focus">
          <div className="d18-drill__head">
            <GlanceRing glance={focused.glance} size={58} />
            <div><p className="d18-eyebrow d18-eyebrow--quiet">{focused.goal.title}{focused.goal.tag ? ` · ${focused.goal.tag}` : ""} {focused.sample && <DataTag kind="sample" />}</p>
              <p className="d18-drill__glance" data-testid="glance">{focused.phase === "ready" ? focused.glance.text : focused.phase === "planning" ? "Amara is planning the route" : "Route appears once the roadmap store answers"}</p>
              {currentStation(focused.stations) && <p className="d18-note">You are here · {currentStation(focused.stations)!.title} · {SIZE_LABEL[currentStation(focused.stations)!.size]}</p>}
            </div>
          </div>
          <div className={`d18-drill__path${growing[focused.goal.id] ? " is-growing" : ""}`} data-cele={cele?.goalId === focused.goal.id ? cele.phase : undefined}>
            <PathSvg drill stations={focused.stations} phase={focused.phase} seed={seedOf(focused.goal.id)} grow={!!growing[focused.goal.id]} branch={branchDraw}
              celebrate={cele && cele.goalId === focused.goal.id ? cele : null} label={pathLabel(focused.goal.title, focused.glance, focused.stations)} />
            {focused.phase === "planning" && <p className="d18-seedline">Amara is planning the route. The path grows in once she has decided the steps.</p>}
            <div className="d18-size-key" aria-label="Station size key"><span><i data-s="small" />Small</span><span><i data-s="medium" />Medium</span><span><i data-s="big" />Big</span></div>
          </div>
          {cele && cele.goalId === focused.goal.id && <p className="d18-amara-line" data-big={cele.big || undefined} role="status">{cele.line} <span className="d18-note">{focused.glance.done} of {focused.glance.total} steps</span></p>}
          {branch && bstate !== "retracting" && bstate !== "declined" && bstate !== "accepted" && <BranchReasonCard b={branch} onAccept={() => { void takeBranch(focused, branch); }} onDecline={() => { void declineBranch(focused, branch); }} />}
          <NextActionCard g={focused} msg={msg} onYes={() => { void yes(focused); }} onNotYet={() => { void notYet(focused); }} onEdit={(t, s) => { void edit(focused, t, s); }} onAccept={() => { void acceptRoute(focused); }} />
          {undo && <div className="d18-toast" role="status">Route changed. <Button size="small" variant="plain" onClick={() => { void undoBranch(); }}>Undo</Button></div>}
        </div>
      )}
      {view !== "focus" && msg && <p className="d18-msg" role="status">{msg}</p>}
    </section>
  );
}

/** Habits under the map: a flame, a 7-day strip, and one shared "Link a source". */
export function HabitsUnderMap({ habits, sample, onLinkSource }: { habits: Habit[]; sample: boolean; onLinkSource?: () => void }) {
  const linked = habits.some((h) => h.days.some((d) => d !== "none"));
  return (
    <section className="d18-card d18-habits" aria-label="Habits this week">
      <header className="d18-map__head"><h2 className="d18-h3">Habits this week</h2>{sample && <DataTag kind="sample" />}</header>
      {habits.length === 0 ? <p className="d18-note">Habits you add appear here and count themselves from their source.</p> : (
        <ul className="d18-habits__list">
          {habits.map((h) => {
            const known = h.days.some((d) => d !== "none");
            let streak = 0; for (let i = h.days.length - 1; i >= 0 && h.days[i] === "met"; i--) streak++;
            return (
              <li key={h.id} className="d18-habit">
                <span className="d18-flame" data-on={streak > 0 || undefined} aria-hidden="true"><Flame size={15} /></span>
                <span className="d18-habit__t"><b>{h.title}</b><small>{h.source} · {known ? h.rule : "daily · no reading yet"}</small></span>
                <span className="d18-grow" />
                <span className="d18-habit__strip"><Dots days={h.days.slice(-7)} label={`${h.title}, last 7 days`} /><small>{streak > 0 ? `${streak} day streak` : "No streak yet"}</small></span>
              </li>
            );
          })}
        </ul>
      )}
      {!linked && habits.length > 0 && (
        <div className="d18-linkrow"><p className="d18-note">Amara counts these from a source. The 7-day strip and the flame fill in once a source is linked.</p><span className="d18-grow" />
          <Button size="small" variant="primary" onClick={onLinkSource} disabled={!onLinkSource}>Link a source</Button></div>
      )}
    </section>
  );
}
