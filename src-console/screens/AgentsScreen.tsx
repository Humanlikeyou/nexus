import type { ReactNode } from "react";
import { Bot, Database, RotateCcw, Users, Wrench } from "lucide-react";
import { INITIAL_SNAPSHOT } from "../bridge";
import type { SystemSnapshot } from "../types";
import { Badge, Button, InsetGroup, ListRow, More, PageHero, type Tone } from "../ui";
import { AgentOrbs, TodaysPulse, deliveryLine } from "../design1118/Agents";
import { PageTake } from "../design1118/PageTake";
import { GoldenUnderlay } from "../design1118/GoldenUnderlay";
import { HermesSubtitle } from "./agentLabels";
import "./AgentsScreen.css";

const cap = (s: string) => (s ? s.charAt(0).toUpperCase() + s.slice(1) : s);
const READY = new Set(["ready", "enabled", "available", "online", "running", "ok"]);

/** Crews row value: running count when the service is up, else its state. */
export function crewsValue(crew: SystemSnapshot["crew"]): string {
  if (crew.health === "not_configured") return "Not configured";
  if (crew.health === "offline") return "Offline";
  const running = crew.runs.filter((r) => r.status === "running").length;
  return `${running} running`;
}

type AgentRow = { id: string; icon: typeof Bot; title: string; subtitle: ReactNode; tone: Tone; label: string };

/** Agent runtimes shown on the Agents page: Hermes (Amara). */
export function agentRows(data: SystemSnapshot, _loading?: boolean): AgentRow[] {
  const hermes = data.hermes;
  const online = hermes.status === "online";
  const hermesUnavailable = data.unavailableSources.some((s) => s.command === "get_terrence_runtime_health");
  return [
    { id: "hermes", icon: Bot, title: "Amara", subtitle: hermesUnavailable ? "Status unavailable" : <HermesSubtitle model={hermes.model} />, tone: online ? "success" : "warning", label: online ? "Online" : hermesUnavailable ? "Unknown" : "Offline" },
  ];
}

/**
 * design-1110b B3: Agents overview. Chat moved to Home (under the lens), so this page is who is
 * running, what they can use, and (below, as a section) what they did: Activity.
 */
export function AgentsField({ data, onOpenCrews, onRetry }: { data: SystemSnapshot; onOpenCrews?: () => void; onRetry?: () => void }) {
  const loading = data.refreshedAt === INITIAL_SNAPSHOT.refreshedAt;
  const rows = agentRows(data, loading);
  const online = rows.filter((r) => r.label === "Online").length;
  const crewReady = data.crew.health !== "offline" && data.crew.health !== "not_configured";
  const kb = data.kb;
  const unreachable = rows.some((r) => r.label === "Not reachable");
  return (
    <div className="agents page-inset d18-page d18-golden">
      <GoldenUnderlay />
      <PageHero title="Agents" statement={loading ? "Checking" : `${online} online`}
        detail={loading ? undefined : `${data.hermes.activeSchedules} schedule${data.hermes.activeSchedules === 1 ? "" : "s"} active · crews ${crewReady ? "ready" : "unavailable"} · ${deliveryLine(data.outbox, data.unavailableSources.some((s) => s.command === "get_outbox_status")).toLowerCase()}`} />
      <PageTake surface="agents" />
      {!loading && <AgentOrbs data={data} />}
      {!loading && <TodaysPulse data={data} />}
      <More label="Agents, tools and knowledge base">
      <InsetGroup title="Agents">
        {rows.map((a) => <ListRow key={a.id} icon={a.icon} title={a.title} subtitle={a.subtitle} accessory={<Badge tone={a.tone} aria-label={`${a.title} status: ${a.label}`}>{a.label}</Badge>} />)}
        {onOpenCrews && <ListRow icon={Users} title="Crews" value={crewsValue(data.crew)} onSelect={onOpenCrews} />}
      </InsetGroup>
      {unreachable && onRetry && (
        <div className="agents__unavailable" role="status">
          <Button variant="secondary" size="small" icon={RotateCcw} onClick={onRetry}>Retry</Button>
        </div>
      )}
      <InsetGroup title="Tools" footer="What Amara can use when she works for you.">
        {data.capabilities.map((c) => (
          <ListRow key={c.name} icon={Wrench} title={c.description || c.name}
            accessory={<Badge tone={READY.has(c.status.toLowerCase()) ? "success" : "neutral"}>{cap(c.status)}</Badge>} />
        ))}
        <ListRow icon={Database} title="Knowledge base" subtitle={[kb.docs != null ? `${kb.docs} documents` : null, kb.model].filter(Boolean).join(" · ") || undefined}
          accessory={<Badge tone={kb.state === "running" ? "success" : kb.state === "error" ? "danger" : "neutral"}>{kb.state === "not_running" ? "Not running" : cap(kb.state)}</Badge>} />
      </InsetGroup>
      </More>
    </div>
  );
}

export default AgentsField;
