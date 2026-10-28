import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

export type ProductTone = "success" | "warning" | "danger" | "neutral" | "accent";

export function StatusIndicator({ tone = "neutral", children }: { tone?: ProductTone; children: ReactNode }) {
  return <span className={`status-indicator status-indicator--${tone}`}><i aria-hidden="true" />{children}</span>;
}

export function PageHeading({ eyebrow, title, description, actions }: { eyebrow?: string; title: string; description?: string; actions?: ReactNode }) {
  return <header className="page-heading">
    <div>{eyebrow && <span className="page-eyebrow">{eyebrow}</span>}<h1>{title}</h1>{description && <p>{description}</p>}</div>
    {actions && <div className="page-actions">{actions}</div>}
  </header>;
}

export function SectionHeading({ title, detail, action }: { title: string; detail?: string; action?: ReactNode }) {
  return <header className="section-heading"><div><h2>{title}</h2>{detail && <p>{detail}</p>}</div>{action}</header>;
}

export function EmptyState({ icon: Icon, title, detail, action }: { icon: LucideIcon; title: string; detail: string; action?: ReactNode }) {
  return <div className="empty-state"><Icon size={24} aria-hidden="true" /><div><h2>{title}</h2><p>{detail}</p></div>{action}</div>;
}

// Migrated: ActionButton is now ui/Button (legacy "danger"/"quiet" variants still accepted).
export { ActionButton } from "../ui/Button";

export function SegmentedControl<T extends string>({ value, options, onChange, label }: { value: T; options: Array<{ value: T; label: string }>; onChange: (value: T) => void; label: string }) {
  return <div className="segmented-control" role="group" aria-label={label}>{options.map((option) => <button key={option.value} type="button" className={value === option.value ? "is-selected" : ""} aria-pressed={value === option.value} onClick={() => onChange(option.value)}>{option.label}</button>)}</div>;
}

export function Metric({ label, value, unit, detail }: { label: string; value: ReactNode; unit?: string; detail?: string }) {
  return <div className="metric"><span>{label}</span><strong>{value}{unit && <small>{unit}</small>}</strong>{detail && <p>{detail}</p>}</div>;
}
