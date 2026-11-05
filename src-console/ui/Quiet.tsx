import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";
import "./Quiet.css";

/**
 * The page language, taken from the lens page: one statement, a handful of things, and everything
 * else one quiet step away. A page says what matters in a sentence before it shows any data.
 */
export function PageHero({ title, statement, detail }: { title: ReactNode; statement: ReactNode; detail?: ReactNode }) {
  return (
    <header className="page-hero">
      <h1 className="page-hero__title">{title}</h1>
      <p className="page-hero__statement">{statement}</p>
      {detail && <p className="page-hero__detail">{detail}</p>}
    </header>
  );
}

/** At most a few plain lines, separated by hairlines. No boxes. */
export function Lines({ label, children }: { label?: string; children: ReactNode }) {
  return <ul className="quiet-lines" aria-label={label}>{children}</ul>;
}
export function Line({ title, detail, trailing }: { title: ReactNode; detail?: ReactNode; trailing?: ReactNode }) {
  return (
    <li className="quiet-line">
      <span className="quiet-line__text"><span className="quiet-line__title">{title}</span>{detail && <span className="quiet-line__detail">{detail}</span>}</span>
      {trailing && <span className="quiet-line__trailing">{trailing}</span>}
    </li>
  );
}

/** Up to four numbers set in a row: no tiles, no borders. */
export function Figures({ children }: { children: ReactNode }) { return <dl className="quiet-figures">{children}</dl>; }
export function Figure({ label, value }: { label: ReactNode; value: ReactNode }) {
  return <div className="quiet-figure"><dt>{label}</dt><dd>{value}</dd></div>;
}

/** The one quiet step away. Closed by default, so a page's first glance stays short. */
export function More({ label, children, id, open }: { label: string; children: ReactNode; id?: string; open?: boolean }) {
  return (
    <details className="quiet-more" id={id} open={open}>
      <summary><ChevronRight className="quiet-more__chevron" aria-hidden="true" />{label}</summary>
      <div className="quiet-more__body">{children}</div>
    </details>
  );
}
