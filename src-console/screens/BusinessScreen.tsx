import { useState } from "react";
import { ventureRoute, type Route } from "../app/routes";
import { businessAdapter, type BusinessAdapter } from "../business/adapters";
import type { SystemSnapshot } from "../types";
import { DataTag } from "../ui";
import { TreasuryView } from "../design1118/TreasuryView";
import { BusinessOverview, LinkCard, VenturesView } from "./BusinessPanels";
import { ContentView, DecisionsView, TasksView, workStatement } from "./EnterprisePages";
import "./BusinessScreen.css";

export { workStatement };

/** D-B6 + design-1111v: Overview's Tasks and Content are one-line link cards to their own tabs. */
function WorkCards({ data, onView }: { data: SystemSnapshot; onView?: (view: string) => void }) {
  const tasksDown = data.unavailableSources.some((s) => s.command === "read_tasks") && data.tasks.length === 0;
  const open = data.tasks.filter((t) => t.status === "active" || t.status === "waiting").slice(0, 2);
  const pipeline = data.pipeline;
  const drafting = pipeline.filter((p) => p.stage === "drafting").length, ideas = pipeline.filter((p) => p.stage === "ideation").length;
  return (
    <div className="cc-grid2">
      <LinkCard title={tasksDown ? "Tasks" : `Tasks · ${workStatement(data.tasks)}`} line={tasksDown ? "Task source unavailable" : open.length ? open.map((t) => `${t.title} is ${t.status === "active" ? "active" : "waiting"}`).join(" · ") : "No open tasks."} open="Open Tasks ›" onOpen={onView ? () => onView("tasks") : undefined} />
      <LinkCard title={pipeline.length ? `Content · ${[drafting ? `${drafting} drafting` : null, ideas ? `${ideas} idea${ideas === 1 ? "" : "s"}` : null].filter(Boolean).join(", ") || `${pipeline.length} items`}` : "Content"} line={pipeline.length ? pipeline.slice(0, 2).map((p) => `${p.title} · ${p.stage === "ideation" ? "idea" : p.stage}`).join(" · ") : "Pipeline is clear."} open="Open Content ›" onOpen={onView ? () => onView("content") : undefined} tag={pipeline.length ? null : <DataTag kind="wire" />} />
    </div>
  );
}

/** Enterprise host (was Business; design-1110d D-B1, design-1111v): Overview · Decisions · Ventures · Tasks · Content
 *  render here; Finances is RevenueField. A venture is its own route, "ventures/<id>". */
export function BusinessField({ data, onRetry, view = "main", onView, onOpen, adapter = businessAdapter(data) }: { data: SystemSnapshot; onRetry?: () => void; view?: string; onView?: (view: string) => void; onOpen?: (route: Route) => void; adapter?: BusinessAdapter }) {
  // Without a router (unit tests) a venture opens in place; in the app it is its own route with a back link.
  const [local, setLocal] = useState<string | null>(null);
  const routed = view.startsWith("ventures/") ? view.slice("ventures/".length) : null;
  const openVenture = (id: string | null) => {
    if (onOpen) onOpen(id ? ventureRoute(id) : { id: "enterprise", view: "ventures" });
    else setLocal(id);
  };
  if (view === "main") return <BusinessOverview data={data} adapter={adapter} onView={onView} onOpenVenture={(id) => openVenture(id)} work={<WorkCards data={data} onView={onView} />} />;
  // "decisions/<itemId>" opens Decisions with that item highlighted.
  if (view === "decisions" || view.startsWith("decisions/")) return <DecisionsView data={data} adapter={adapter} onOpen={onOpen} focusId={view.startsWith("decisions/") ? view.slice("decisions/".length) : undefined} />;
  if (view === "ventures" || routed) return <VenturesView data={data} adapter={adapter} ventureId={routed ?? local} onOpen={openVenture} onView={onView} onRoute={onOpen} />;
  if (view === "treasury") return <TreasuryView />;
  if (view === "content") return <ContentView data={data} onRetry={onRetry} onOpen={onOpen} />;
  return <TasksView data={data} onRetry={onRetry} onOpen={onOpen} />;
}
