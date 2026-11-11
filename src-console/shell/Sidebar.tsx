import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import type { ModuleId, SystemSnapshot } from "../types";
import { SYSTEM_GROUPS, SYSTEMS } from "../app/routes";
import { StatusIndicator } from "../design/ProductUI";
import { standaloneAdapter } from "../standalone/adapters";
import { servicesFooter } from "../standalone/servicesSummary";
import "./shell.css";

export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 280;
export const SIDEBAR_DEFAULT = 220;
const STORAGE_KEY = "nexus.sidebarWidth";

export const clampSidebar = (w: number) => Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, w)));

function storedWidth(): number {
  try {
    const raw = Number(window.localStorage.getItem(STORAGE_KEY));
    return Number.isFinite(raw) && raw > 0 ? clampSidebar(raw) : SIDEBAR_DEFAULT;
  } catch {
    return SIDEBAR_DEFAULT;
  }
}

/** Sidebar width (200–280, default 220), persisted and applied as --shell-sidebar-w. */
function useSidebarWidth() {
  const [width, setWidth] = useState(storedWidth);
  useEffect(() => {
    document.documentElement.style.setProperty("--shell-sidebar-w", `${width}px`);
    try { window.localStorage.setItem(STORAGE_KEY, String(width)); } catch { /* private mode */ }
  }, [width]);
  return [width, (w: number) => setWidth(clampSidebar(w))] as const;
}

function Resizer({ width, setWidth }: { width: number; setWidth: (w: number) => void }) {
  const drag = useRef<{ x: number; w: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const onPointerDown = useCallback((e: PointerEvent<HTMLDivElement>) => {
    drag.current = { x: e.clientX, w: width };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    setDragging(true);
  }, [width]);
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (drag.current) setWidth(drag.current.w + e.clientX - drag.current.x);
  };
  const end = () => { drag.current = null; setDragging(false); };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? 20 : 10;
    if (e.key === "ArrowLeft") { e.preventDefault(); setWidth(width - step); }
    else if (e.key === "ArrowRight") { e.preventDefault(); setWidth(width + step); }
    else if (e.key === "Home") { e.preventDefault(); setWidth(SIDEBAR_MIN); }
    else if (e.key === "End") { e.preventDefault(); setWidth(SIDEBAR_MAX); }
  };
  return (
    // WAI-ARIA window-splitter pattern: a focusable separator with a value is interactive, but jsx-a11y treats role="separator" as static.
    /* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
    <div
      className="shell-sidebar-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={SIDEBAR_MAX}
      aria-valuenow={width}
      tabIndex={0}
      data-dragging={dragging || undefined}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={end}
      onPointerCancel={end}
      onDoubleClick={() => setWidth(SIDEBAR_DEFAULT)}
      onKeyDown={onKeyDown}
    />
    /* eslint-enable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
  );
}

export function Sidebar({ active, onSelect, data, onOpenServices }: { active: ModuleId; onSelect: (id: ModuleId) => void; data: SystemSnapshot; onOpenServices?: () => void }) {
  const [width, setWidth] = useSidebarWidth();
  // design-1111s: the footer becomes a live service summary once services_summary exists (sample in preview).
  const summary = servicesFooter(standaloneAdapter());
  return (
    <nav className="system-constellation" aria-label="Amara OS navigation">
      {SYSTEM_GROUPS.map((group) => (
        <section className="nav-group" key={group.label || group.systems.map((s) => s.id).join("+")} aria-label={group.label || undefined}>
          {group.label && <h2>{group.label}</h2>}
          {group.systems.map((system) => {
            const Icon = system.icon;
            const index = SYSTEMS.findIndex((entry) => entry.id === system.id);
            const isActive = active === system.id;
            return (
              <button key={system.id} data-system={system.id} className={isActive ? "is-active" : ""} aria-current={isActive ? "page" : undefined} onClick={() => onSelect(system.id)} title={`${system.label}${index < 9 ? ` (Command+${index + 1})` : ""}`}>
                <i>{system.id === "terrance" ? <b className="nav-lens" aria-hidden="true" /> : <Icon size={16} strokeWidth={1.6} />}</i><span>{system.label}</span>{index < 9 && <small className="nav-shortcut" aria-hidden="true">⌘{index + 1}</small>}
              </button>
            );
          })}
        </section>
      ))}
      {summary && onOpenServices
        ? <div className="sidebar-status"><button type="button" className="sidebar-status__link" onClick={onOpenServices} aria-label={`${summary.text}. Open Settings, Services`}><StatusIndicator tone={summary.attention ? "warning" : "accent"}>{summary.text} ›</StatusIndicator></button></div>
        : <div className="sidebar-status" title={data.hermes.model}><StatusIndicator tone={data.hermes.status === "online" ? "success" : "danger"}>{data.hermes.status === "online" ? "Amara ready" : "Amara offline"}</StatusIndicator><span className="ui-visually-hidden">{data.hermes.model}</span></div>}
      <Resizer width={width} setWidth={setWidth} />
    </nav>
  );
}
