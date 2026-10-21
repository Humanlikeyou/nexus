import type { ModuleId, SystemSnapshot } from "../types";
import { SYSTEM_GROUPS, SYSTEMS } from "../app/routes";
import { StatusIndicator } from "../design/ProductUI";

export function Sidebar({ active, onSelect, data }: { active: ModuleId; onSelect: (id: ModuleId) => void; data: SystemSnapshot }) {
  const setActive = onSelect;
  return (
    <nav className="system-constellation" aria-label="Nexus navigation">
      {SYSTEM_GROUPS.map((group) => <section className="nav-group" key={group.label}><h2>{group.label}</h2>{group.systems.map((system) => { const Icon = system.icon; const index = SYSTEMS.findIndex((entry) => entry.id === system.id); return <button key={system.id} className={active === system.id ? "is-active" : ""} aria-current={active === system.id ? "page" : undefined} onClick={() => setActive(system.id)} title={`${system.label}${index < 9 ? ` (Command+${index + 1})` : ""}`}><i><Icon size={17} /></i><span>{system.label}</span><small>{system.signal}</small></button>; })}</section>)}
      <div className="sidebar-status"><StatusIndicator tone={data.hermes.status === "online" ? "success" : "danger"}>{data.hermes.status === "online" ? "Nexus ready" : "Nexus offline"}</StatusIndicator><span>{data.hermes.model}</span></div>
    </nav>
  );
}
