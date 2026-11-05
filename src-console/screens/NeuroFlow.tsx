import { Volume2, VolumeX } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { deriveElectrodeMode, isOutputAllowed } from "../../src/lib/electrode/electrodePolicy";
import { useAudioFeedback } from "../../src/hooks/useAudioFeedback";
import { loadKv, saveKv } from "../bridge";
import { AmaraLens } from "../components/AmaraLens/AmaraLens";
import { GOALS, NOTES_KEY, SENSORS, goalOf, parseNotes, recommendedGoal, suggestedStrategy, type NfNote } from "../neuro/goals";
import { FIT_LABEL, HOLD_COPY, feedbackLevel, type ChannelFit } from "../neuro/training";
import type { MuseTelemetry } from "../neuro/telemetryStore";
import type { useTraining } from "../neuro/useTraining";
import { useNow } from "../shell/useNow";
import type { RecordedSession } from "../types";
import { Button, Figure, Figures, SegmentedControl, Sparkline } from "../ui";
import "./NeuroFlow.css";

type Training = ReturnType<typeof useTraining>;
/** The light should breathe with the brain, not flicker with each reading. */
const LENS_SMOOTHING = { attackMs: 420, releaseMs: 1300 };
const seconds = (value: number) => (value >= 60 ? `${Math.round(value / 60)} min` : `${Math.round(value)} s`);

/** Real AUX samples, or nothing. Output stays locked: isOutputAllowed() is false by policy. */
function ElectrodeLine({ aux }: { aux: MuseTelemetry["aux"] }) {
  const now = useNow(1_000).getTime();
  if (!aux) return null;
  const mode = deriveElectrodeMode(true, now - aux.lastPacketAt < 3_000, "live", 1);
  return <p className="nf-electrode" data-mode={mode}><i aria-hidden="true" />Myndlift electrode: {mode === "read" ? "reading" : "no signal"}{isOutputAllowed() ? "" : ", read only"}</p>;
}

function Lens({ state, level }: { state: "idle" | "thinking" | "speaking"; level: number }) {
  return <div className="nf-lens"><AmaraLens state={state} size={320} level={level} smoothing={LENS_SMOOTHING} decorative /></div>;
}

function useNotes() {
  const [notes, setNotes] = useState<NfNote[]>([]);
  useEffect(() => {
    let live = true;
    loadKv(NOTES_KEY).then((raw) => { if (live) setNotes(parseNotes(raw)); }, () => undefined);
    return () => { live = false; };
  }, []);
  const add = (note: NfNote) => {
    const next = [note, ...notes].slice(0, 200);
    setNotes(next);
    saveKv(NOTES_KEY, JSON.stringify(next)).catch(() => undefined);
  };
  return { notes, add };
}

function GoalStep({ training, aux, lastNote, action }: { training: Training; aux: MuseTelemetry["aux"]; lastNote: NfNote | null; action: ReactNode }) {
  const protocol = training.protocols.find((item) => item.id === training.protocolId) ?? null;
  const goal = protocol ? goalOf(protocol.id) : null;
  const recommended = recommendedGoal(!!aux);
  const options = [...GOALS.map((item) => ({ value: item.id, label: item.title })), { value: "record", label: "Just record" }];
  return (
    <section className="nf-step" aria-label="Choose a goal">
      <h2 className="nf-step__title">What would you like from this session?</h2>
      <SegmentedControl label="Goal" value={goal?.id ?? "record"} options={options}
        onChange={(id) => training.setProtocolId(id === "record" ? "off" : GOALS.find((item) => item.id === id)!.protocolIds[0])} />
      {goal && protocol ? (
        <>
          <p className="nf-step__line">{goal.line}{recommended === goal.id && <span className="nf-recommended"> Recommended with your electrode.</span>}</p>
          {goal.protocolIds.length > 1 && (
            <SegmentedControl label="Session type" size="small" value={protocol.id} onChange={training.setProtocolId}
              options={goal.protocolIds.map((id) => ({ value: id, label: training.protocols.find((item) => item.id === id)?.name ?? id }))} />
          )}
          <p className="nf-strategy"><span>Try this</span>{suggestedStrategy(protocol)}</p>
          <p className="nf-meta">{protocol.defaultDurationMin} minutes{protocol.eyesClosed ? " · Eyes closed, soft sound guides you" : " · Watch the light, or close your eyes"}</p>
        </>
      ) : <p className="nf-step__line">Records your session with no feedback.</p>}
      {lastNote && <p className="nf-meta">Last time: {lastNote.note}</p>}
      <ElectrodeLine aux={aux} />
      <div className="nf-actions">{action}</div>
      <p className="nf-note">For relaxation and attention training. It isn't a medical treatment.</p>
    </section>
  );
}

function FitStep({ fit, ready, onContinue, aux, action }: { fit: Record<string, ChannelFit>; ready: boolean; onContinue: () => void; aux: MuseTelemetry["aux"]; action: ReactNode }) {
  return (
    <section className="nf-step" aria-label="Headband fit">
      <h2 className="nf-step__title">Settle the headband</h2>
      <p className="nf-step__line">Sit still and let the sensors find your skin. Moisten the forehead sensors if one stays poor.</p>
      <ul className="nf-fit" aria-label="Sensor contact">
        {SENSORS.map(({ channel, name }) => {
          const quality = fit[channel] ?? "none";
          return <li key={channel} data-quality={quality}><i aria-hidden="true" /><span>{name}</span><span className="nf-fit__state">{FIT_LABEL[quality]}</span></li>;
        })}
      </ul>
      <ElectrodeLine aux={aux} />
      <div className="nf-actions">
        <Button variant="primary" disabled={!ready} onClick={onContinue}>{ready ? "Begin baseline" : "Waiting for good contact"}</Button>
        {action}
      </div>
    </section>
  );
}

function BaselineStep({ closed, percent, hold, action }: { closed: boolean; percent: number; hold: string | null; action: ReactNode }) {
  return (
    <section className="nf-live" aria-label="Baseline">
      <Lens state="thinking" level={0} />
      <p className="nf-live__label">{closed ? "Close your eyes" : "Eyes open, look softly at the light"}</p>
      <p className="nf-live__sub" role="status">{hold ?? (closed ? "A soft bell will tell you when it's done." : "Twenty seconds, then eyes closed. This is your own starting point.")}</p>
      <div className="nf-progress" role="progressbar" aria-label="Baseline" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent)}><i style={{ transform: `scaleX(${percent / 100})` }} /></div>
      <div className="nf-actions nf-actions--center">{action}</div>
    </section>
  );
}

function TrainingStep({ training, soundOn, onSound, action }: { training: Training; soundOn: boolean; onSound: () => void; action: ReactNode }) {
  const { state } = training;
  const level = feedbackLevel(state);
  const protocol = training.protocols.find((item) => item.id === training.protocolId);
  const held = state.phase === "held";
  return (
    <section className="nf-live" aria-label="Live feedback" data-held={held || undefined}>
      <Lens state="speaking" level={level} />
      <p className="nf-live__label" role="status" aria-live="polite">{held ? "Feedback held" : state.label}</p>
      <p className="nf-live__sub">{state.hold ? HOLD_COPY[state.hold] : protocol ? suggestedStrategy(protocol) : ""}</p>
      <dl className="nf-readout">
        <div><dt>In target</dt><dd>{state.targetPercent == null ? "—" : `${Math.round(state.targetPercent)}%`}</dd></div>
        <div><dt>Delay</dt><dd>{state.delayMs == null ? "—" : `${(state.delayMs / 1000).toFixed(1)} s`}</dd></div>
        {state.auxSource && <div><dt>Source</dt><dd>Myndlift electrode</dd></div>}
      </dl>
      <div className="nf-actions nf-actions--center">
        <Button variant="plain" size="small" icon={soundOn ? Volume2 : VolumeX} aria-pressed={soundOn} onClick={onSound}>{soundOn ? "Sound on" : "Sound off"}</Button>
        {action}
      </div>
    </section>
  );
}

function SummaryStep({ training, sessions, onNote, lastStrategy }: { training: Training; sessions: RecordedSession[]; onNote: (note: string) => void; lastStrategy: string }) {
  const done = training.finished!;
  const guided = done.guided!;
  const [note, setNote] = useState("");
  const [saved, setSaved] = useState(false);
  const target = done.time_in_target_pct;
  const held = Object.values(guided.held_seconds).reduce((sum, value) => sum + value, 0);
  const worst = (Object.entries(guided.held_seconds) as Array<[keyof typeof HOLD_COPY, number]>).sort((a, b) => b[1] - a[1])[0];
  const trend = useMemo(() => [...sessions].sort((a, b) => a.start.localeCompare(b.start)).map((session) => session.summary.time_in_target_pct).filter((value): value is number => typeof value === "number").slice(-12), [sessions]);
  const ratio = guided.alpha_eyes_closed_ratio;
  return (
    <section className="nf-step" aria-label="Session summary">
      <h2 className="nf-step__title">{done.protocol_name}</h2>
      <p className="nf-summary__figure">{target == null ? "—" : `${Math.round(target)}%`}<span>of clean time in your target</span></p>
      <Figures>
        <Figure label="Trained" value={seconds(guided.trained_seconds)} />
        <Figure label="Held" value={seconds(held)} />
        <Figure label="Threshold" value={`${Math.round(guided.reward_schedule.threshold_scale * 100)}%`} />
      </Figures>
      {held > 0 && worst[1] > 0 && <p className="nf-meta">Most of the held time was {worst[0] === "blink" ? "blinks" : worst[0] === "jaw" ? "jaw tension" : worst[0] === "movement" ? "movement" : worst[0] === "dropout" ? "signal dropouts" : "poor contact"}. Held time is never counted.</p>}
      {ratio !== null && <p className="nf-meta">{ratio >= 1.2 ? `Alpha rose ${ratio.toFixed(1)}× with your eyes closed, a good sign the signal was clean.` : "Alpha barely changed with your eyes closed, so the fit may have been loose."}</p>}
      {trend.length >= 2 && (
        <div className="nf-trend"><h3>Time in target, last {trend.length} sessions</h3><Sparkline values={trend} label={`Time in target, last ${trend.length} sessions`} format={(value) => `${Math.round(value)}%`} size="lg" /></div>
      )}
      <form className="nf-noteform" onSubmit={(event) => { event.preventDefault(); if (note.trim()) { onNote(note.trim()); setSaved(true); } }}>
        <label htmlFor="nf-note">What helped? <span>You were offered: {lastStrategy}</span></label>
        <textarea id="nf-note" rows={2} value={note} onChange={(event) => { setNote(event.target.value); setSaved(false); }} placeholder="Where your attention went, what you tried" />
        <div className="nf-actions">
          <Button type="submit" variant="secondary" disabled={!note.trim() || saved}>{saved ? "Saved" : "Save note"}</Button>
          <Button variant="plain" onClick={training.dismissFinished}>Done</Button>
        </div>
      </form>
    </section>
  );
}

/** The session itself: goal, fit, baseline, training, summary. The lens is the feedback. */
export function NeuroFlow({ training, telemetry, sessions, running, action }: { training: Training; telemetry: MuseTelemetry; sessions: RecordedSession[]; running: boolean; action: ReactNode }) {
  const { state } = training;
  const { notes, add } = useNotes();
  const { init, setMode, cleanup, setMuted, playBell, updateState } = useAudioFeedback();
  const protocol = training.protocols.find((item) => item.id === training.protocolId);
  const [soundOn, setSoundOn] = useState(false);
  const stage = running ? state.stage : undefined;
  const prev = useRef(stage);
  const strategy = protocol ? suggestedStrategy(protocol) : "";

  // Sound can only start from his click, which is the session's Start button.
  const armed = useRef(false);
  useEffect(() => {
    if (!running || armed.current) return;
    armed.current = true;
    setSoundOn(!!protocol?.eyesClosed);
    try { init(); setMode("tone"); } catch { /* no audio on this machine: the light still works */ }
  }, [running, protocol, init, setMode]);
  useEffect(() => { if (!running && armed.current) { armed.current = false; try { cleanup(); } catch { /* nothing to close */ } } }, [running, cleanup]);
  useEffect(() => { setMuted(!soundOn); }, [soundOn, setMuted]);

  // Bells carry the session when his eyes are closed.
  useEffect(() => {
    if (prev.current !== stage && (prev.current === "baseline-open" || prev.current === "baseline-closed")) {
      try { playBell(prev.current === "baseline-open" ? "countdown" : "start"); } catch { /* the words still show */ }
    }
    prev.current = stage;
  }, [stage, playBell]);
  useEffect(() => {
    if (stage !== "training") return;
    try { updateState("calm", state.phase === "training" && state.signal?.action === "reward"); } catch { /* the light still works */ }
  }, [stage, state.phase, state.signal, updateState]);

  if (training.finished && !running) return <SummaryStep training={training} sessions={sessions} lastStrategy={strategy} onNote={(note) => add({ at: new Date().toISOString(), protocol: training.finished!.protocol_name, targetPercent: training.finished!.time_in_target_pct, strategy, note })} />;
  if (!running || !state.stage) {
    if (running) return <div className="nf-actions">{action}</div>; // a plain recording: the live readings below are the screen
    return <GoalStep training={training} aux={telemetry.aux} lastNote={notes[0] ?? null} action={action} />;
  }
  if (state.stage === "fit") return <FitStep fit={state.fit ?? {}} ready={!!state.fitReady} onContinue={training.advance} aux={telemetry.aux} action={action} />;
  if (state.stage !== "training") return <BaselineStep closed={state.stage === "baseline-closed"} percent={state.calibrationPercent} hold={state.hold ? HOLD_COPY[state.hold] : null} action={action} />;
  return <TrainingStep training={training} soundOn={soundOn} onSound={() => setSoundOn((on) => !on)} action={action} />;
}

