import { useMemo, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Button, Card, PageHero, wiredTag } from "../ui";
import type { SystemSnapshot } from "../types";
import { recordingsAdapter, type RecordingsAdapter } from "../neuro/recordings";
import { RecordingDetail, RecordingsList, RetentionCard } from "./RecordingsPanels";
import "./SessionsScreen.css";

/** Sessions → Mind–Body › Neuro history. design-1110e: Muse recordings, every channel, 90-day confirm-only retention. */
const SESSIONS_COMMAND = "list_sessions";

export function SessionsField({ data, onRetry, onStartSession, adapter: given }: { data: SystemSnapshot; onRetry?: () => void; onStartSession?: () => void; adapter?: RecordingsAdapter }) {
  const adapter = useMemo(() => given ?? recordingsAdapter(data.sessions), [given, data.sessions]);
  const list = adapter.list();
  const items = list.state === "unwired" ? [] : list.value;
  const [openId, setOpenId] = useState<string | null>(null);
  const failed = data.unavailableSources.some((s) => s.command === SESSIONS_COMMAND);
  const open = items.find((r) => r.id === openId) ?? null;

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
  if (open) return <RecordingDetail item={open} adapter={adapter} onBack={() => setOpenId(null)} />;

  const totalMin = Math.round(items.reduce((sum, r) => sum + r.durationSecs / 60, 0));
  return (
    <div className="sessions page-inset rec-page">
      <PageHero title="Neuro history" statement={`${items.length} recording${items.length === 1 ? "" : "s"}, ${totalMin} min`} detail={<span className="rec-hero-line">Muse · newest first {wiredTag(list)}</span>} />
      <RecordingsList list={list} onOpen={setOpenId} />
      <RetentionCard adapter={adapter} />
    </div>
  );
}
