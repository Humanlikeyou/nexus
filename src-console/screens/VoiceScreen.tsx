import { Play, RefreshCw, RotateCcw, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { getVoiceStatus, getVoiceTurns, listVoices, setVoice, startVoice, stopVoice } from "../bridge";
import { dateTime, plainText } from "../lib/format";
import type { VoiceList, VoiceState, VoiceStatus, VoiceTurn } from "../types";
import { Badge, Button, Card, InsetGroup, ListRow, More, PageHero, Select, type Tone } from "../ui";
import { AmaraLens } from "../components/AmaraLens/AmaraLens";
import { useAmaraVoice, useAmaraVoiceBridge } from "../hooks/useAmaraVoice";
import "./VoiceScreen.css";

export const VOICE_STATUS_POLL_MS = 1500;
export const VOICE_TURNS_POLL_MS = 4000;
export const VOICE_TURNS = 20;

/** What each logs/state word means to Taisun, its tone, and whether the dot pulses (live activity). */
export const STATE_VIEW: Record<VoiceState, { label: string; detail: string; tone: Tone; pulse: boolean }> = {
  loading: { label: "Loading", detail: "Loading Whisper and her voice. The first start takes about 2 minutes.", tone: "info", pulse: true },
  idle: { label: "Ready", detail: "Say “Amara” to talk to her.", tone: "success", pulse: false },
  listening: { label: "Listening", detail: "Follow-up window: answer without saying her name.", tone: "accent", pulse: true },
  heard: { label: "Heard you", detail: "Transcribing.", tone: "accent", pulse: true },
  thinking: { label: "Thinking", detail: "Amara is working on the answer.", tone: "info", pulse: true },
  speaking: { label: "Speaking", detail: "Say “Amara” or “stop” to cut her off.", tone: "accent", pulse: true },
  error: { label: "Error", detail: "The last turn failed (Hermes or her voice). See recent turns.", tone: "danger", pulse: false },
  stopped: { label: "Stopped", detail: "Not listening. The mic is free.", tone: "neutral", pulse: false },
  not_installed: { label: "Not installed", detail: "", tone: "neutral", pulse: false },
};

const ROUTE_BADGE: Record<string, { tone: Tone; text: string }> = {
  hermes: { tone: "info", text: "Amara" },
  local: { tone: "success", text: "Local" },
  repeat: { tone: "warning", text: "Asked to repeat" },
  ignored: { tone: "neutral", text: "Ignored" },
};

function errorText(e: unknown): string {
  return plainText(String(e instanceof Error ? e.message : e)).slice(0, 300);
}

const secs = (v: number | null | undefined) => (typeof v === "number" && Number.isFinite(v) ? `${v.toFixed(1)} s` : null);

/** "ASR 0.4 s · first audio 5.4 s" (whatever the turn recorded). */
export function turnLatency(t: VoiceTurn): string {
  return [
    secs(t.asr_s) && `ASR ${secs(t.asr_s)}`,
    secs(t.token_s) && `first token ${secs(t.token_s)}`,
    secs(t.first_audio_s) && `first audio ${secs(t.first_audio_s)}`,
    !t.first_audio_s && secs(t.total_s) && `done ${secs(t.total_s)}`,
  ].filter(Boolean).join(" · ");
}

function TurnRow({ turn }: { turn: VoiceTurn }) {
  const route = ROUTE_BADGE[turn.route ?? ""] ?? { tone: "neutral" as Tone, text: turn.route ?? "Turn" };
  const said = turn.text || turn.heard || "(nothing)";
  const reply = turn.route === "ignored" ? `Not acted on: ${turn.ignored ?? turn.verdict ?? "unsure"}` : turn.error ? `Failed: ${turn.error}` : turn.reply || "(no reply)";
  return (
    <ListRow
      title={<span className="voice__said">{said}</span>}
      subtitle={<span className="voice__reply">{reply}</span>}
      value={[turn.time ? dateTime(turn.time) : null, turnLatency(turn) || null].filter(Boolean).join(" · ")}
      accessory={<span className="voice__badges">
        {turn.barged && <Badge tone="warning">{turn.barged === "stop" ? "Stopped" : "Interrupted"}</Badge>}
        <Badge tone={turn.error ? "danger" : route.tone}>{route.text}</Badge>
      </span>}
    />
  );
}

/** Chat & Agents › Voice: the Amara Voice loop's live state, start/stop, her voice, and recent spoken turns. */
export function VoiceField() {
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [turns, setTurns] = useState<VoiceTurn[]>([]);
  const [voices, setVoices] = useState<VoiceList | null>(null);
  const [busy, setBusy] = useState<"start" | "stop" | "restart" | "voice" | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  /** Voice the running loop started with; a different saved choice needs a restart. */
  const [runningVoice, setRunningVoice] = useState<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const lens = useAmaraVoice();
  useAmaraVoiceBridge();

  const refreshStatus = useCallback(async () => {
    try {
      const s = await getVoiceStatus();
      if (!mounted.current) return;
      setStatus(s);
      setStatusError(null);
      setRunningVoice((prev) => (s.running ? prev ?? s.voice : null));
    } catch (e) {
      if (mounted.current) setStatusError(errorText(e));
    }
  }, []);
  const refreshTurns = useCallback(async () => {
    try {
      const t = await getVoiceTurns(VOICE_TURNS);
      if (mounted.current) setTurns(t);
    } catch { /* turns are optional; status shows real failures */ }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- initial load + polling of an external process (amara-voice), as in useSnapshot
    void refreshStatus();
    void refreshTurns();
    listVoices().then((v) => { if (mounted.current) setVoices(v); }).catch(() => { /* picker hidden */ });
    const a = window.setInterval(() => { void refreshStatus(); }, VOICE_STATUS_POLL_MS);
    const b = window.setInterval(() => { void refreshTurns(); }, VOICE_TURNS_POLL_MS);
    return () => { window.clearInterval(a); window.clearInterval(b); };
  }, [refreshStatus, refreshTurns]);

  const run = async (kind: "start" | "stop" | "restart", action: () => Promise<VoiceStatus>) => {
    setBusy(kind);
    setActionError(null);
    try {
      const s = await action();
      if (!mounted.current) return;
      setStatus(s);
      setRunningVoice(s.running ? s.voice : null);
    } catch (e) {
      if (mounted.current) setActionError(errorText(e));
    } finally {
      if (mounted.current) setBusy(null);
    }
  };

  const pickVoice = async (name: string) => {
    setBusy("voice");
    setActionError(null);
    try {
      const v = await setVoice(name);
      if (mounted.current) setVoices(v);
    } catch (e) {
      if (mounted.current) setActionError(errorText(e));
    } finally {
      if (mounted.current) setBusy(null);
    }
  };

  if (!status) {
    return (
      <div className="voice page-inset">
        <PageHero title="Voice" statement={statusError ? "Out of reach" : "Checking"} />
        <Card>
          {statusError ? (
            <div className="voice__unavailable">
              <p>Voice status unavailable: {statusError}</p>
              <Button variant="secondary" size="small" icon={RefreshCw} onClick={() => { void refreshStatus(); }}>Retry</Button>
            </div>
          ) : <p className="voice__note" role="status">Checking Amara Voice…</p>}
        </Card>
      </div>
    );
  }

  if (!status.installed) {
    return (
      <div className="voice page-inset">
        <PageHero title="Voice" statement="Amara Voice isn’t set up on this Mac"
          detail={`Talk to Amara out loud with the local voice loop. Amara OS looks for it in ${status.dir}.`} />
      </div>
    );
  }

  const view = STATE_VIEW[status.state] ?? STATE_VIEW.stopped;
  const current = voices?.current ?? status.voice;
  const needsRestart = status.running && runningVoice !== null && current !== runningVoice;
  const recent = [...turns].reverse();
  return (
    <div className="voice page-inset">
      <PageHero title="Voice" statement={<span aria-label={`Amara Voice: ${view.label}`}>{view.label}</span>} detail={<span role="status" aria-live="polite">{view.detail}</span>} />
      <div className="voice__live" data-state={status.state}>
        <div className="voice__lens"><AmaraLens state={lens.state} size={160} showLabel />{lens.loading && <span className="voice__lens-note" role="status">Loading voice model…</span>}</div>
        <div className="voice__actions">
          {status.running
            ? <Button variant="secondary" icon={Square} loading={busy === "stop"} disabled={busy !== null} onClick={() => { void run("stop", stopVoice); }}>Stop</Button>
            : <Button variant="primary" icon={Play} loading={busy === "start"} disabled={busy !== null} onClick={() => { void run("start", startVoice); }}>Start</Button>}
        </div>
        {statusError && <p className="voice__error" role="alert">Status unavailable: {statusError}</p>}
        {actionError && <p className="voice__error" role="alert">{actionError}</p>}
      </div>

      <More label="Her voice, process and recent turns">
      <dl className="voice__facts">
        <div><dt>Voice</dt><dd>{status.voice}</dd></div>
        {status.running && <div><dt>Memory</dt><dd>{status.rss_mb != null ? `${Math.round(status.rss_mb).toLocaleString()} MB` : "--"}</dd></div>}
        {status.running && <div><dt>Process</dt><dd>{status.pid ?? "--"}</dd></div>}
        <div><dt>Runs as</dt><dd>{status.launchd ? "Login item (launchd)" : "App process"}</dd></div>
      </dl>
      {voices && voices.voices.length > 0 && (
        <Card title="Her voice">
          <div className="voice__picker">
            <Select label="Voice" value={current} disabled={busy !== null}
              options={voices.voices.map((v) => ({ value: v, label: v }))}
              description="Reference clips in amara-voice/voices (a .wav with its .txt). Used from the next start."
              onChange={(e) => { void pickVoice(e.target.value); }} />
            {needsRestart && (
              <div className="voice__restart">
                <p className="voice__note">Still speaking as {runningVoice}. Restart to switch to {current}.</p>
                <Button variant="secondary" size="small" icon={RotateCcw} loading={busy === "restart"} disabled={busy !== null}
                  onClick={() => { void run("restart", async () => { await stopVoice(); return startVoice(); }); }}>Restart</Button>
              </div>
            )}
          </div>
        </Card>
      )}

      {recent.length === 0
        ? <Card title="Recent turns"><p className="voice__note"><strong>No turns yet</strong><br />Say “Amara, what time is it?” and the turn shows up here with its timings.</p></Card>
        : <InsetGroup title="Recent turns">{recent.map((t, i) => <TurnRow key={`${t.time ?? ""}-${i}`} turn={t} />)}</InsetGroup>}
      </More>
    </div>
  );
}
