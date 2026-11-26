import { useEffect, useRef, useState } from "react";
import { invokeBound } from "../bridge";
import { Button, Card, PageHero, Select, StatusText } from "../ui";
import { CalibrationRun, CHANNELS, DEFAULT_SECS, FEATURE_LABEL, REJECT_LABEL, type CalibrationState, type ConditionResult, type RejectReason } from "./calibration";
import { useMuseTelemetry } from "./useMuseTelemetry";
import "../ui/PanelKit.css";

/** Decision 0058: Neuro › Calibration (design boundary lifted for this view only). Optional; never blocks a session. */
type Stat = { median: number; robustSd: number; n: number };
type Comparison = { condition: string; feature: string; value: number; usualMedian: number; usualRobustSd: number; robustZ: number | null; position: string };
type CondBase = { condition: string; recordings: number; relBands: Record<string, Stat>; alphaPeakHz: Stat | null; confidence: string; uncertainty: string };
type Baseline = { version: string; conditions: CondBase[]; alphaReactivity: Stat | null; note: string };
type PrePostChange = { condition: string; feature: string; pre: number; post: number; change: number; usualRobustSd: number | null; changeInUsualSpreads: number | null; beyondUsualVariation: boolean };
type PrePost = { preReportId: string; minutesBetween: number; changes: PrePostChange[]; note: string };
type Saved = { report: { id: string; createdAt: string }; baselineEligible: Record<string, boolean>; comparison: Comparison[]; baselineBefore: Baseline; prePost?: PrePost | null };
type Call = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

const DURATIONS = [20, 30, 60, 90, 120, 180, 300].map((s) => ({ value: String(s), label: s < 60 ? `${s} seconds` : `${s / 60} minute${s === 60 ? "" : "s"}` }));
const COND_LABEL: Record<string, string> = { eyes_open: "Eyes open", eyes_closed: "Eyes closed" };
const FIT_TEXT: Record<string, string> = { good: "Good contact", fair: "Fair contact", poor: "Poor contact", none: "No signal" };
const POSITION: Record<string, string> = { within: "within your usual range", above: "above your usual range", below: "below your usual range" };
const pct = (v: number) => `${Math.round(v * 100)}%`;
const fmt = (feature: string, v: number) => (feature === "alpha_peak_hz" ? `${v.toFixed(1)} Hz` : feature === "variability" ? v.toFixed(3) : `${v.toFixed(1)}%`);

export const NOT_A_STATE = "These are measurements of the EEG signal. They are not measures of emotion, attention, stress or relaxation.";

function ConditionReport({ r, comparison, eligible }: { r: ConditionResult; comparison: Comparison[]; eligible: boolean | undefined }) {
  const rejected = Object.entries(r.rejected).filter(([, n]) => (n ?? 0) > 0);
  const f = r.features;
  const rows: Array<[string, number | null]> = f ? [...(["delta", "theta", "alpha", "beta", "gamma"] as const).map((k) => [k, f.relBands[k] ?? null] as [string, number | null]), ["alpha_peak_hz", f.alphaPeakHz], ["variability", f.variability]] : [];
  return (
    <Card title={COND_LABEL[r.condition]} headingLevel={3}>
      <p className="pk-note">{r.epochsAccepted} of {r.epochsTotal} one-second epochs kept ({Math.round(r.recordedSecs)} of {r.plannedSecs} s recorded).
        {rejected.length > 0 && ` Rejected: ${rejected.map(([k, n]) => `${n} ${REJECT_LABEL[k as RejectReason]}`).join(", ")}.`}
        {eligible === true ? " Good enough to count toward your baseline." : eligible === false ? " Not good enough to count toward your baseline (it is still saved)." : ""}</p>
      <p className="pk-note">Contact over the recording: {CHANNELS.map((c) => `${c} ${pct(r.channelQuality[c] ?? 0)}`).join(" · ")}</p>
      {!f ? <p className="pk-note">No epoch was clean enough to measure.</p> : (
        <table className="pk-table" aria-label={`${COND_LABEL[r.condition]} measurements`}>
          <thead><tr><th scope="col">Measure</th><th scope="col">This time</th><th scope="col">Compared with your history</th></tr></thead>
          <tbody>{rows.map(([k, v]) => {
            const c = comparison.find((x) => x.condition === r.condition && x.feature === k);
            return <tr key={k}><td>{FEATURE_LABEL[k]}</td><td>{v == null ? (k === "alpha_peak_hz" ? "Not estimable" : "Not measured") : fmt(k, v)}</td>
              <td>{c ? `${POSITION[c.position]} (usual ${fmt(k, c.usualMedian - c.usualRobustSd)}–${fmt(k, c.usualMedian + c.usualRobustSd)})` : "No baseline yet"}</td></tr>;
          })}</tbody>
        </table>
      )}
    </Card>
  );
}

/** Decision 0059: before and after a session, feature by feature. A measured change, never a verdict. */
export function PrePostCard({ prePost }: { prePost: PrePost | null | undefined }) {
  if (!prePost) return <Card title="Before and after"><p className="pk-note">There is no resting recording from before this session (from the last six hours), so there is nothing to compare with. The recording is still saved.</p></Card>;
  const signed = (feature: string, v: number) => `${v > 0 ? "+" : v < 0 ? "−" : "±"}${fmt(feature, Math.abs(v))}`;
  return (
    <Card title="Before and after">
      <p className="pk-note">Compared with your resting recording from {prePost.minutesBetween} minutes earlier.</p>
      {(["eyes_open", "eyes_closed"] as const).map((cond) => {
        const rows = prePost.changes.filter((c) => c.condition === cond);
        if (!rows.length) return null;
        return (
          <table key={cond} className="pk-table" aria-label={`${COND_LABEL[cond]} before and after`}>
            <thead><tr><th scope="col">{COND_LABEL[cond]}</th><th scope="col">Before</th><th scope="col">After</th><th scope="col">Change</th></tr></thead>
            <tbody>{rows.map((c) => <tr key={c.feature}><td>{FEATURE_LABEL[c.feature]}</td><td>{fmt(c.feature, c.pre)}</td><td>{fmt(c.feature, c.post)}</td>
              <td>{signed(c.feature, c.change)} · {c.changeInUsualSpreads == null ? "no baseline to judge by" : c.beyondUsualVariation ? "larger than your usual variation" : "within your usual variation"}</td></tr>)}</tbody>
          </table>
        );
      })}
      <p className="pk-note">{prePost.note}</p>
    </Card>
  );
}

export function BaselineCard({ base }: { base: Baseline | null }) {
  if (!base) return null;
  return (
    <Card title="Your resting baseline" accessory={<span className="pk-acc"><span className="v-note">{base.version}</span></span>}>
      {base.conditions.map((c) => (
        <p key={c.condition} className="pk-note"><b>{COND_LABEL[c.condition]}</b>: {c.relBands.alpha ? `alpha ${c.relBands.alpha.median.toFixed(1)}% (usual ${(c.relBands.alpha.median - c.relBands.alpha.robustSd).toFixed(1)}–${(c.relBands.alpha.median + c.relBands.alpha.robustSd).toFixed(1)}%)` : "no baseline yet"}
          {c.alphaPeakHz ? ` · alpha peak ${c.alphaPeakHz.median.toFixed(1)} Hz` : ""} · confidence {c.confidence}. {c.uncertainty}</p>
      ))}
      {base.alphaReactivity && <p className="pk-note">Eyes-closed alpha is usually {base.alphaReactivity.median.toFixed(2)}× eyes-open (n={base.alphaReactivity.n}).</p>}
      <p className="pk-note">{base.note}</p>
    </Card>
  );
}

/** Pre-meditation calibration: check, eyes open, eyes closed, report. Separate from a protocol's own calibration. */
export function CalibrationView({ onDone, call = invokeBound as Call, now = Date.now, after = false }: { onDone?: () => void; call?: Call; now?: () => number; /** Decision 0059: the optional rest recording after a session. */ after?: boolean }) {
  const telemetry = useMuseTelemetry();
  const [secs, setSecs] = useState(DEFAULT_SECS);
  const run = useRef<CalibrationRun | null>(null);
  const [state, setState] = useState<CalibrationState | null>(null);
  const [saved, setSaved] = useState<Saved | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [base, setBase] = useState<Baseline | null>(null);
  useEffect(() => { call<Baseline>("neuro_baseline_get").then(setBase, () => setBase(null)); }, [call]);
  // A stream this view started (no session, no recording) is stopped when the view closes.
  const ownStream = useRef(false);
  const [streamErr, setStreamErr] = useState<string | null>(null);
  useEffect(() => () => { if (ownStream.current) void call("stop_eeg_stream").catch(() => {}); }, [call]);
  // Only a stream this view really started is its to stop: never one a session (or anything else) already runs.
  const startStream = () => {
    setStreamErr(null);
    call<{ streaming?: boolean } | null>("get_muse_state").then(async (st) => {
      if (st?.streaming) return;
      await call("start_eeg_stream");
      ownStream.current = true;
    }).catch(() => setStreamErr("The headset stream didn't start. Connect the Muse on the Neuro page first."));
  };
  useEffect(() => {
    if (!run.current) return;
    const s = run.current.tick(telemetry, now());
    setState(s);
    if (s.stage === "done" && !saved && !err) {
      const report = run.current.report(after ? "post_session" : "pre_meditation");
      run.current = null;
      if (!report.conditions.length) return;
      call<Saved>("neuro_calibration_save", { report }).then((r) => { setSaved(r); return call<Baseline>("neuro_baseline_get").then(setBase); }, () => setErr("The calibration couldn't be saved. Nothing was changed."));
    }
    // Each new telemetry snapshot is one tick.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [telemetry]);
  const startCheck = () => { run.current = new CalibrationRun(secs); setSaved(null); setErr(null); setState(run.current.tick(telemetry, now())); };
  const stage = state?.stage;
  return (
    <div className="v-page" id="calibration">
      {after
        ? <PageHero title="After-session rest" statement="A resting recording after your session" detail="Optional. The same eyes-open and eyes-closed recording, compared with the one from before your session. It is kept apart from your baseline." />
        : <PageHero title="Calibration" statement="A resting recording before you meditate" detail="Optional. It measures your resting EEG with eyes open and eyes closed, and builds a personal baseline over time. You can start a session without it." />}
      <BaselineCard base={base} />
      {!state || (stage === "done" && (saved || err)) ? (
        <Card title={saved ? "Calibrate again" : "Before you start"}>
          <p className="pk-note">Sit comfortably with the Muse on and streaming. You&apos;ll keep your eyes open for the first part and closed for the second. Stay still; blinks and movement are left out automatically.</p>
          <Select label="Length of each part" options={DURATIONS} value={String(secs)} onChange={(e) => setSecs(Number(e.target.value))} />
          <div className="pk-actions"><Button size="small" variant="primary" onClick={startCheck}>Check the headset</Button>{onDone && <Button size="small" variant="plain" onClick={onDone}>{after ? "Skip" : "Skip calibration"}</Button>}</div>
        </Card>
      ) : stage === "check" ? (
        <Card title="Checking the headset">
          <ul className="pk-list" aria-label="Channel contact">{CHANNELS.map((c) => <li key={c}>{c}: <StatusText kind={state.channelFit[c] === "good" ? "ok" : state.channelFit[c] === "fair" ? "wait" : "fail"}>{FIT_TEXT[state.channelFit[c] ?? "none"]}</StatusText></li>)}</ul>
          <p className="pk-note" role="status">{state.blocker ?? "Every sensor has contact. Keep your eyes open and look at one point."}</p>
          {telemetry.packetCount === 0 && <div className="pk-actions"><Button size="small" variant="secondary" onClick={startStream}>Start the headset stream</Button></div>}
          {streamErr && <p className="pk-note" role="alert">{streamErr}</p>}
          <div className="pk-actions"><Button size="small" variant="primary" disabled={!!state.blocker} onClick={() => { if (run.current?.begin()) setState(run.current.state); }}>Start eyes-open recording</Button>
            <Button size="small" variant="plain" onClick={() => { run.current = null; setState(null); }}>Cancel</Button></div>
        </Card>
      ) : stage === "eyes_open" || stage === "eyes_closed" ? (
        <Card title={stage === "eyes_open" ? "Eyes open" : "Eyes closed"}>
          <p className="pk-note" role="status">{stage === "eyes_open" ? "Keep your eyes open and rest your gaze on one point." : "Close your eyes and rest. A tone is not played."} {Math.round(state.secs)} of {state.plannedSecs} s · {state.accepted} kept · {state.rejected} left out</p>
          <div className="pk-actions"><Button size="small" variant="secondary" onClick={() => { run.current?.stop(); if (run.current) setState({ ...run.current.state }); }}>Stop and keep what was recorded</Button></div>
        </Card>
      ) : null}
      {err && <p className="pk-note" role="alert">{err}</p>}
      {saved && state && (
        <section aria-label="Calibration report">
          {after && <PrePostCard prePost={saved.prePost} />}
          {state.results.map((r) => <ConditionReport key={r.condition} r={r} comparison={saved.comparison} eligible={after ? undefined : saved.baselineEligible[r.condition]} />)}
          {after && <p className="pk-note">After-session recordings are kept apart from your baseline.</p>}
          <p className="pk-note">{NOT_A_STATE}</p>
          {onDone && <div className="pk-actions"><Button size="small" variant="primary" onClick={onDone}>{after ? "Done" : "Continue to a session"}</Button></div>}
        </section>
      )}
    </div>
  );
}
