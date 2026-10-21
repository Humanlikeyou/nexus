import { tone, humanize, plainText } from "../lib/format";
import type { SystemSnapshot } from "../types";
import { State } from "./shared";
import { BriefcaseBusiness, SquarePen } from "lucide-react";

export function BusinessField({ data }: { data: SystemSnapshot }) {
  const states = ["active", "backlog", "waiting", "done"];
  const stages = ["ideation", "drafting", "review", "scheduled", "published"];
  return (
    <div className="system-field business-field">
      <section className="ops-heading"><span className="system-kicker">Work</span><h1>Business</h1><p>{data.tasks.length} tracked work items</p></section>
      <section className="ops-pulse">{states.map((state) => <div key={state}><span>{state}</span><strong>{data.tasks.filter((task) => task.status === state).length}</strong><i className={`tone-${tone(state)}`} /></div>)}</section>
      <section className="work-ledger"><header><span>Work item</span><span>Priority</span><span>Tags</span><span>Status</span></header>{data.tasks.length > 0 ? data.tasks.map((task, index) => <article key={`${task.title}-${index}`}><strong>{plainText(task.title)}</strong><span>{task.priority ? humanize(task.priority) : "Normal"}</span><span>{task.tags?.map(humanize).join(" · ") || "--"}</span><State value={task.status} /></article>) : <div className="workspace-empty"><BriefcaseBusiness size={24} aria-hidden="true" /><strong>No work items yet</strong><p>Tracked priorities will collect here as they are created.</p></div>}</section>
      <section className="production-arc"><header><span>Content production</span><strong>{data.pipeline.length}</strong></header>{data.pipeline.length > 0 ? <div>{stages.map((stage) => { const items = data.pipeline.filter((item) => item.stage === stage); return <section key={stage}><header><span>{humanize(stage)}</span><strong>{items.length}</strong></header>{items.slice(0, 5).map((item, index) => <article key={`${item.title}-${index}`}><strong>{plainText(item.title)}</strong><span>{humanize(item.content_type)}</span></article>)}</section>; })}</div> : <div className="workspace-empty workspace-empty--compact"><SquarePen size={24} aria-hidden="true" /><strong>Pipeline is clear</strong><p>Content moves through this view from idea to publication.</p></div>}</section>
    </div>
  );
}
