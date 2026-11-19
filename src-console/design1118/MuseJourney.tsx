import { useRef, useState, type ReactNode } from "react";
import { hoursMinutes } from "../lib/mindBodyDeep";
import { parseBodyContext } from "../lib/noop";
import { BANDS, hasSamples, score, sessionStats, type SessionWithSamples } from "../neuro/sessionSamples";
import "../neuro/Recordings.css";
import type { RecordedSession } from "../types";
import { Button } from "../ui";
import { PHI, phiWave } from "./golden";
import { smoothPath } from "./pathModel";
import { neuroStrings, EEG_SLOWDOWN, LOOP_S } from "./tangleString";
import { VibratingStrings, tangleGlAvailable } from "./TangleWaves";
import "./design1118.css";

/**
 * design-1118a Neuro › Muse (round 5): the last session is the hero (a journey, not a table). Recovery
 * THAT DAY (from the session's saved body context) and recovery NOW (today's WHOOP via NOOP) are two
 * separate rings and are never mixed. One next action: Find Muse. No Legacy & Systems question here.
 */
const whenFmt = new Intl.DateTimeFormat("en-US", { weekday: "short", month: "short", day: "numeric" });
const dayFmt = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" });
const hhmm = (iso: string) => new Date(iso).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
const whole = (v: number | null | undefined) => (typeof v === "number" && Number.isFinite(v) ? Math.round(v) : null);

function Ring({ value, size, tone, children, label }: { value: number | null; size: number; tone: "mint" | "copper"; children: ReactNode; label: string }) {
  const r = size / 2 - 7, c = 2 * Math.PI * r, frac = value == null ? 0 : Math.max(0, Math.min(1, value / 100));
  return (
    <div className="d18-mring" data-tone={tone} style={{ width: size, height: size }} role="img" aria-label={label}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <circle className="d18-mring__track" cx={size / 2} cy={size / 2} r={r} />
        <circle className="d18-mring__fill" cx={size / 2} cy={size / 2} r={r} strokeDasharray={`${(frac * c).toFixed(1)} ${c.toFixed(1)}`} transform={`rotate(-90 ${size / 2} ${size / 2})`} />
      </svg>
      <div className="d18-mring__v" aria-hidden="true">{children}</div>
    </div>
  );
}

/** Decorative session visualizer: calm in mint, focus in copper, shaped by the session averages. Not the raw trace. */
function SessionWave({ focus, calm }: { focus: number | null; calm: number | null }) {
  // golden character: focus runs φ faster than calm, each wave carries a φ² smaller overtone, phases a golden angle apart
  const strand = (amp: number, k: number, seed: number) => smoothPath(Array.from({ length: 72 }, (_, i) => { const t = i / 71; return { x: 4 + t * 632, y: 40 + phiWave(t, k, amp, seed) }; }));
  return (
    <svg className="d18-swave" viewBox="0 0 640 80" aria-hidden="true">
      <path className="d18-swave__calm" d={strand(8 + (calm ?? 30) / 6, 3.4, 1)} />
      <path className="d18-swave__focus" d={strand(8 + (focus ?? 30) / 6, 3.4 * PHI, 2)} />
    </svg>
  );
}

/**
 * Decision 0041: the same vibrating strings as Milestones, one per brainwave band of the most recent session.
 * Each band's string runs at its real centre frequency slowed 40x (so delta to gamma keep their true ratios), its
 * swing set by how strong that band was. Every string makes whole cycles per minute, so the picture loops
 * seamlessly and only changes when a new session is saved. No band data: calm (alpha) and focus (beta) scores.
 */
function NeuroWave({ session, focus, calm }: { session: RecordedSession; focus: number | null; calm: number | null }) {
  const [gl, setGl] = useState(tangleGlAvailable);
  const keyRef = useRef<HTMLDivElement>(null);
  const s = session as SessionWithSamples;
  const bandAvg = hasSamples(s) ? sessionStats(s.samples).bandAvg : null;
  const strings = neuroStrings({ bandAvg, calm, focus });
  if (!gl || strings.length === 0) return <><SessionWave focus={focus} calm={calm} /><p className="d18-note">Session visualizer · calm in mint, focus in copper · not the raw trace</p></>;
  const named = (k: string) => BANDS.find((b) => b.key === k);
  const colourOf = (i: number) => {
    const sw = keyRef.current?.querySelector<HTMLElement>(`[data-band="${strings[i]?.key}"] i`);
    return sw ? getComputedStyle(sw).backgroundColor : null;
  };
  return (
    <>
      <VibratingStrings strings={strings.map((x) => x.string)} hot={null} colourOf={colourOf} onFail={() => setGl(false)} viewW={640} viewH={80} swing={0.4}
        className="d18-swave" testId="neuro-strings" label={`Your last session's brainwaves: ${strings.map((x) => named(x.key)?.name ?? x.key).join(", ")}, each at its own frequency`} />
      <div className="rec-legend" ref={keyRef}>
        {strings.map((x) => <span key={x.key} data-band={x.key} style={{ ["--c" as string]: `var(--band-${x.key})` }}><i aria-hidden="true" />{named(x.key)?.name} <span>{named(x.key)?.range}</span></span>)}
      </div>
      <p className="d18-note">{bandAvg ? "Your last session's brainwaves" : "Calm (alpha) and focus (beta) from your last session"} · each band vibrates at its own frequency, {EEG_SLOWDOWN}× slower · stronger bands swing wider · loops every {LOOP_S === 60 ? "minute" : `${LOOP_S} s`} until your next session</p>
    </>
  );
}

export function MuseJourney({ session, recoveryNow, sessions, action, status, devices, onOpenHistory, onOpenSession, hideNext = false }: {
  session: RecordedSession | null; recoveryNow: number | null; sessions: RecordedSession[]; action: ReactNode; /** Status chip, left of the primary button on the compact Find Muse row. */ status?: ReactNode; /** Nearby headsets and scan notes; render only while scanning or when there is something to say. */ devices?: ReactNode; onOpenHistory?: () => void;
  /** A session chip or the hero opens that recording. */ onOpenSession?: (sessionId: string) => void;
  /** Last-session hero only (connected and idle: the page already has its own next step). */ hideNext?: boolean;
}) {
  const body = session ? parseBodyContext(session.body_context) : null;
  const that = whole(body?.recovery);
  const now = whole(recoveryNow);
  const focus = session ? score(session.summary.avg_focus) : null;
  const calm = session ? score(session.summary.avg_relaxation) : null;
  const minutes = session ? Math.max(1, Math.round(session.duration_secs / 60)) : 0;
  return (
    <div className="d18-muse">
      {session ? (
        <section className="d18-card d18-muse__hero" aria-label="Last session">
          <div className="d18-muse__rings">
            <Ring value={that} size={150} tone="mint" label={`Last session ${whenFmt.format(new Date(session.start))}, ${minutes} min. Recovery that day ${that == null ? "not recorded" : `${that}%`}`}>
              <span className="d18-eyebrow">Last session</span>
              <b className="d18-mring__big">{that == null ? "—" : `${that}%`}</b>
              <small>Recovery that day</small>
              <small className="d18-mring__when"><b>{whenFmt.format(new Date(session.start))} · {minutes} min</b><br />{hhmm(session.start)}–{hhmm(session.end)}</small>
            </Ring>
            {now != null && (
              <Ring value={now} size={104} tone="copper" label={`Recovery now ${now}%`}>
                <span className="d18-eyebrow">Now</span><b className="d18-mring__mid">{now}%</b><small>Recovery now</small>
              </Ring>
            )}
          </div>
          <NeuroWave session={session} focus={focus} calm={calm} />
          <div className="d18-muse__stats">
            <Ring value={focus} size={60} tone="copper" label={`Focus ${focus ?? "not recorded"}`}><b>{focus ?? "—"}</b><small>focus</small></Ring>
            <Ring value={calm} size={60} tone="mint" label={`Calm ${calm ?? "not recorded"}`}><b>{calm ?? "—"}</b><small>calm</small></Ring>
            {body?.hrv_ms != null && <span className="d18-stat"><b>{Math.round(body.hrv_ms)}<small> ms</small></b><small>HRV</small></span>}
            {body?.resting_hr != null && <span className="d18-stat"><b>{Math.round(body.resting_hr)}<small> bpm</small></b><small>Resting HR</small></span>}
            {body?.sleep_minutes != null && <span className="d18-stat"><b>{hoursMinutes(body.sleep_minutes)}</b><small>Sleep before</small></span>}
          </div>
          <div className="d18-muse__strip">
            {sessions.slice(0, 4).map((s) => { const b = parseBodyContext(s.body_context); const open = onOpenSession ? { role: "button" as const, tabIndex: 0, "aria-label": `Open the ${dayFmt.format(new Date(s.start))} recording`, onClick: () => onOpenSession(s.id), onKeyDown: (e: React.KeyboardEvent) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onOpenSession(s.id); } } } : {}; return <span key={s.id} className="d18-muse__chip" {...open}><i aria-hidden="true" /><b>{dayFmt.format(new Date(s.start))}</b><small>{Math.max(1, Math.round(s.duration_secs / 60))} min{b?.recovery != null ? ` · ${Math.round(b.recovery)}%` : ""}</small></span>; })}
            <span className="d18-muse__more"><span>Next sessions will line up here.</span>{onOpenSession && <Button size="small" variant="plain" onClick={() => onOpenSession(session.id)}>Open recording</Button>}{onOpenHistory && <Button size="small" variant="plain" onClick={onOpenHistory}>Session history</Button>}</span>
          </div>
          <p className="d18-note">{session.file_path ? "The raw signal of this session is saved on this Mac." : "This session has its summary only."}</p>
        </section>
      ) : (
        <section className="d18-card d18-muse__hero" aria-label="Sessions">
          <p className="d18-note">Your first Muse session appears here as a journey.</p>
          {now != null && <Ring value={now} size={104} tone="copper" label={`Recovery now ${now}%`}><span className="d18-eyebrow">Now</span><b className="d18-mring__mid">{now}%</b><small>Recovery now</small></Ring>}
        </section>
      )}
      {!hideNext && <section className="d18-next" aria-label="Next step">
        <div className="d18-next__row">
          {status}
          <p className="d18-next__why">Put your headset in pairing mode</p>
          <div className="d18-actions">{action}</div>
        </div>
        {devices}
      </section>}
    </div>
  );
}
