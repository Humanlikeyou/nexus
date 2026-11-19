import { useEffect, useMemo, useState } from "react";
import { RefreshCw } from "lucide-react";
import { loadKv } from "../bridge";
import { parseBodyContext } from "../lib/noop";
import { useStoresTick } from "../lib/localStores";
import { Button, Card, PageHero, wiredTag } from "../ui";
import type { SystemSnapshot } from "../types";
import type { RecordingsAdapter } from "../neuro/recordings";
import { useRecordingsAdapter } from "../neuro/useRecordingsAdapter";
import { hemisyncAdapter } from "../neuro/hemisync/live";
import type { HemisyncAdapter } from "../neuro/hemisync/adapters";
import { sessionNoteKey } from "../neuro/goals";
import { RecordingDetail, RecordingsList, RetentionCard, type Filter, type RecordingContext } from "./RecordingsPanels";
import "./SessionsScreen.css";

/** Sessions → Mind–Body › Neuro history. design-1110e: Muse recordings, every channel, 90-day confirm-only retention. */
const SESSIONS_COMMAND = "list_sessions";

export type SessionsFieldProps = {
  data: SystemSnapshot; onRetry?: () => void; onStartSession?: () => void; adapter?: RecordingsAdapter;
  /** A session / recording id to open (Neuro's "Open recording ›", the journey chips, a route like neuro-history/<id>). */
  initialOpenId?: string | null;
  /** Back from a recording: the page clears the id from its route so the same recording can be opened again. */
  onClose?: () => void;
  /** Overrides the Hemi-Sync adapter (tests). */
  hemisync?: HemisyncAdapter;
};

const baseName = (path: string | null | undefined) => (path ? path.split("/").pop() ?? null : null);
const sameFile = (a: string | null | undefined, b: string | null | undefined) => { const x = baseName(a), y = baseName(b); return !!x && x === y; };

export function SessionsField({ data, onRetry, onStartSession, adapter: given, initialOpenId = null, onClose, hemisync }: SessionsFieldProps) {
  const live = useRecordingsAdapter(data.sessions, !given);
  const adapter = given ?? live;
  const list = adapter.list();
  const items = list.state === "unwired" ? [] : list.value;
  const [openId, setOpenId] = useState<string | null>(initialOpenId);
  const [filter, setFilter] = useState<Filter>("all");
  // A new id from the route opens that recording; no id (the History tab) shows the list.
  const [seenId, setSeenId] = useState(initialOpenId);
  if (seenId !== initialOpenId) { setSeenId(initialOpenId); setOpenId(initialOpenId); }
  const failed = data.unavailableSources.some((s) => s.command === SESSIONS_COMMAND);
  // A session id opens its recording: live recordings are listed by replay file name, so match through the saved session's file.
  const wanted = data.sessions.find((s) => s.id === openId);
  const open = items.find((r) => r.id === openId || (wanted && sameFile(r.replayFile, wanted.file_path))) ?? null;
  const owner = open ? data.sessions.find((s) => s.id === open.id || sameFile(open.replayFile, s.file_path)) : null;
  const context = useRecordingContext(open ? (owner?.id ?? open.id) : null, data, hemisync);

  if (failed && items.length === 0) {
    return (
      <div className="sessions page-inset">
        <PageHero title="Neuro history" statement="Sensor sessions unavailable" />
        <Card>{onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}</Card>
      </div>
    );
  }
  if (items.length === 0) {
    return (
      <div className="sessions page-inset rec-page">
        <PageHero title="Neuro history" statement="No recordings yet" detail="Muse" />
        <Card title="Your Muse sessions will appear here" headingLevel={2}>
          <p className="sessions__copy">Every session saves the raw signal from each sensor (TP9, AF7, AF8, TP10, and AUX when the back-of-head electrode is detected) on this Mac, so you can scroll back through it later.</p>
          {onStartSession && <div><Button variant="primary" onClick={onStartSession}>Start session</Button></div>}
        </Card>
      </div>
    );
  }
  if (open) return <RecordingDetail item={open} adapter={adapter} context={context} onBack={() => { setOpenId(null); onClose?.(); }} />;

  const totalMin = Math.round(items.reduce((sum, r) => sum + r.durationSecs / 60, 0));
  return (
    <div className="sessions page-inset rec-page">
      <PageHero title="Neuro history" statement={`${items.length} recording${items.length === 1 ? "" : "s"}, ${totalMin} min`} detail={<span className="rec-hero-line">Muse · newest first {wiredTag(list)}</span>} />
      <RecordingsList list={list} onOpen={setOpenId} filter={filter} onFilter={setFilter} />
      <RetentionCard adapter={adapter} />
    </div>
  );
}

/** One recording's Hemi-Sync link, body context and "What helped?" note. Reads only; every part is optional. */
function useRecordingContext(id: string | null, data: SystemSnapshot, given?: HemisyncAdapter): RecordingContext {
  const tick = useStoresTick();
  const adapter = useMemo(() => given ?? hemisyncAdapter(), [given]);
  const [note, setNote] = useState<{ id: string; text: string | null } | null>(null);
  useEffect(() => {
    if (!id) return;
    let live = true;
    loadKv(sessionNoteKey(id)).then((raw) => { if (live) setNote({ id, text: raw && raw.trim() ? raw : null }); }, () => { if (live) setNote({ id, text: null }); });
    return () => { live = false; };
  }, [id]);
  return useMemo(() => {
    if (!id) return { hemisync: null, body: null, note: null };
    const session = data.sessions.find((s) => s.id === id);
    const link = adapter.sessions();
    const linked = link.state === "unwired" ? null : link.value.find((s) => s.id === id) ?? null;
    const lib = adapter.library();
    const track = linked && lib.state !== "unwired" ? lib.value.waves.flatMap((w) => w.tracks).find((t) => t.id === linked.trackId) ?? null : null;
    return {
      hemisync: linked ? { title: track?.title ?? "Hemi-Sync track", wave: track ? `Wave ${track.wave} · ${track.waveName}` : null, rating: linked.rating, note: linked.note } : null,
      body: session ? parseBodyContext(session.body_context) : null,
      note: note?.id === id ? note.text : null,
    };
    // `tick` re-reads the Hemi-Sync adapter after its stores load
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, data.sessions, adapter, note, tick]);
}
