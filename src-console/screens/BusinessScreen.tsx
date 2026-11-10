import { RefreshCw } from "lucide-react";
import { humanize, plainText } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { Badge, Button, Figure, Figures, InsetGroup, Line, Lines, ListRow, More, PageHero } from "../ui";
import { badgeTone } from "./businessTone";
import { useState } from "react";
import { businessAdapter, type BusinessAdapter } from "../business/adapters";
import { BusinessOverview, VenturesView } from "./BusinessPanels";
import { Card, StatusText, type StatusKind } from "../ui";
import "./BusinessScreen.css";

/** Business › Work (screen-specs §7). */
const STATES = ["active", "backlog", "waiting", "done"] as const;
const STAGES = ["ideation", "drafting", "review", "scheduled", "published"] as const;
/** How many items a page shows before the disclosure. */
const FIRST_GLANCE = 3;
type Task = SystemSnapshot["tasks"][number];

function Unavailable({ label, onRetry }: { label: string; onRetry?: () => void }) {
  return (
    <div className="biz__unavailable">
      <p>{label} unavailable</p>
      {onRetry && <Button variant="secondary" size="small" icon={RefreshCw} onClick={onRetry}>Retry</Button>}
    </div>
  );
}

/** Most urgent first: what's moving, then what's stuck, then the most important of the rest. */
const STATUS_RANK: Record<string, number> = { active: 0, waiting: 1, backlog: 2 };
const PRIORITY_RANK: Record<string, number> = { urgent: 0, high: 1 };
const rank = (task: Task) => (STATUS_RANK[task.status] ?? 3) * 10 + (PRIORITY_RANK[task.priority ?? ""] ?? 2);

/** "1 in progress, 1 waiting": the headline state, and nothing else. */
export function workStatement(tasks: Task[]): string {
  const count = (status: string) => tasks.filter((task) => task.status === status).length;
  const active = count("active"), waiting = count("waiting");
  if (active === 0 && waiting === 0) return tasks.length ? "Nothing in progress" : "No work items yet";
  return [active ? `${active} in progress` : "", waiting ? `${waiting} waiting` : ""].filter(Boolean).join(", ");
}

const TASK_KIND: Record<string, StatusKind> = { active: "run", waiting: "wait", backlog: "off", done: "ok" };
const STAGE_TEXT: Record<string, string> = { ideation: "Idea" };

/** D-B6: Overview's bottom row, from the real task and pipeline sources. */
function WorkCards({ data, onRetry }: { data: SystemSnapshot; onRetry?: () => void }) {
  const tasksDown = data.unavailableSources.some((s) => s.command === "read_tasks") && data.tasks.length === 0;
  const open = data.tasks.filter((t) => t.status !== "done").sort((a, b) => rank(a) - rank(b)).slice(0, 4);
  return (
    <div className="cc-grid2">
      <Card title={tasksDown ? "Work" : `Work \u00b7 ${workStatement(data.tasks)}`}>
        {tasksDown ? <Unavailable label="Task source" onRetry={onRetry} /> : open.length === 0 ? <p className="cc-note">No open work.</p> : open.map((t, i) => <div className="cc-row" key={`${t.title}-${i}`}><div className="cc-row__main"><span>{plainText(t.title)}</span><small>{[...(t.tags ?? []), t.priority].filter(Boolean).join(" \u00b7 ") || "Normal"}</small></div><StatusText kind={TASK_KIND[t.status] ?? "off"}>{humanize(t.status)}</StatusText></div>)}
      </Card>
      <Card title="Content pipeline">
        {data.pipeline.length === 0 ? <p className="cc-note">Pipeline is clear.</p> : data.pipeline.slice(0, 4).map((p, i) => <div className="cc-row" key={`${p.title}-${i}`}><div className="cc-row__main"><span>{plainText(p.title)}</span><small>{humanize(p.content_type)}</small></div><StatusText kind={p.stage === "ideation" ? "off" : p.stage === "published" ? "ok" : "run"}>{STAGE_TEXT[p.stage] ?? humanize(p.stage)}</StatusText></div>)}
        <p className="cc-note">Venture links for tasks need wiring.</p>
      </Card>
    </div>
  );
}

/** Business host (design-1110d D-B1): Overview, Ventures and Work render here; Money is RevenueField. */
export function BusinessField({ data, onRetry, view = "main", onView, adapter = businessAdapter() }: { data: SystemSnapshot; onRetry?: () => void; view?: string; onView?: (view: string) => void; adapter?: BusinessAdapter }) {
  const [ventureId, setVentureId] = useState<string | null>(null);
  if (view === "main") return <BusinessOverview data={data} adapter={adapter} onOpenVenture={(id) => { if (onView) { setVentureId(id); onView("ventures"); } }} work={<WorkCards data={data} onRetry={onRetry} />} />;
  if (view === "ventures") return <VenturesView data={data} adapter={adapter} ventureId={ventureId} onOpen={setVentureId} />;
  return <WorkView data={data} onRetry={onRetry} />;
}

/** Business › Work: the state, the top three, and everything else behind one disclosure. */
function WorkView({ data, onRetry }: { data: SystemSnapshot; onRetry?: () => void }) {
  const down = (command: string) => data.unavailableSources.some((s) => s.command === command);
  const tasks = data.tasks;
  const pipeline = data.pipeline;
  const open = tasks.filter((task) => task.status !== "done").sort((a, b) => rank(a) - rank(b));
  const top = open.slice(0, FIRST_GLANCE);
  const tasksDown = down("read_tasks") && tasks.length === 0;
  const pipelineDown = down("read_content_pipeline") && pipeline.length === 0;
  const other = tasks.filter((t) => !(STATES as readonly string[]).includes(t.status)).length;
  return (
    <div className="biz page-inset">
      <PageHero title="Business" statement={tasksDown ? "Work items are out of reach" : workStatement(tasks)}
        detail={tasks.length === 0 && !tasksDown ? "Tracked priorities collect here as they\u2019re created." : undefined} />

      {tasksDown ? <Unavailable label="Task source" onRetry={onRetry} /> : top.length > 0 && (
        <Lines label="Work in focus">
          {top.map((task, i) => <Line key={`${task.title}-${i}`} title={plainText(task.title)} trailing={<Badge tone={badgeTone(task.status)}>{humanize(task.status)}</Badge>} />)}
        </Lines>
      )}

      <More label="All work and content">
        <Figures>
          {STATES.map((state) => <Figure key={state} label={humanize(state)} value={tasks.filter((t) => t.status === state).length} />)}
          {other > 0 && <Figure label="Other" value={other} />}
        </Figures>
        {tasks.length > 0 && (
          <InsetGroup title="Work items" footer={`${tasks.length} tracked`}>
            {tasks.map((task, i) => (
              <ListRow key={`${task.title}-${i}`} title={plainText(task.title)} subtitle={task.tags?.length ? task.tags.map(humanize).join(" · ") : undefined}
                value={task.priority ? humanize(task.priority) : "Normal"} accessory={<Badge tone={badgeTone(task.status)}>{humanize(task.status)}</Badge>} />
            ))}
          </InsetGroup>
        )}
        <section className="biz__pipeline" aria-label="Content pipeline">
          <h2 className="biz__section">Content pipeline · {pipeline.length}</h2>
          {pipelineDown ? <Unavailable label="Content pipeline" onRetry={onRetry} />
            : pipeline.length === 0 ? <p className="biz__note">Pipeline is clear. Content moves here from idea to publication.</p>
              : STAGES.map((stage) => {
                const items = pipeline.filter((item) => item.stage === stage);
                if (items.length === 0) return null;
                return (
                  <InsetGroup key={stage} className="biz__stage" title={`${humanize(stage)} · ${items.length}`}>
                    {items.slice(0, 5).map((item, i) => <ListRow key={`${item.title}-${i}`} title={<span title={item.notes ?? undefined}>{plainText(item.title)}</span>} subtitle={humanize(item.content_type)} />)}
                  </InsetGroup>
                );
              })}
        </section>
      </More>
    </div>
  );
}
