import type { LucideIcon } from "lucide-react";
import { useId, type ComponentPropsWithRef, type ReactNode } from "react";
import { cx } from "./shared";
import "./InsetGroup.css";

export type InsetGroupProps = Omit<ComponentPropsWithRef<"ul">, "title" | "role"> & {
  /** Group header above the rows; the list is labelled by it. */
  title?: ReactNode;
  /** Explanatory text below the rows. */
  footer?: ReactNode;
};

/** macOS-style inset grouped list. `role="list"` keeps list semantics in WebKit despite list-style: none. */
export function InsetGroup({ title, footer, className, children, ...rest }: InsetGroupProps) {
  const id = useId();
  const titleId = title != null ? `${id}-title` : undefined;
  const footerId = footer != null ? `${id}-footer` : undefined;
  return (
    <div className={cx("ui-inset-group", className)}>
      {title != null && <h3 id={titleId} className="ui-inset-group__title">{title}</h3>}
      {/* eslint-disable-next-line jsx-a11y/no-redundant-roles -- WebKit drops list semantics under list-style: none */}
      <ul aria-labelledby={titleId} aria-describedby={footerId} {...rest} role="list" className="ui-inset-group__list">{children}</ul>
      {footer != null && <p id={footerId} className="ui-inset-group__footer">{footer}</p>}
    </div>
  );
}

export type ListRowProps = Omit<ComponentPropsWithRef<"li">, "title" | "onSelect"> & {
  icon?: LucideIcon;
  title: ReactNode;
  subtitle?: ReactNode;
  /** Trailing value text (e.g. "On", "3 devices"). */
  value?: ReactNode;
  /** Trailing control, rendered outside the row button so it stays independently interactive. */
  accessory?: ReactNode;
  /** Makes the row a button. */
  onSelect?: () => void;
  /** Marks the row as the current one (aria-current). Only meaningful with onSelect. */
  selected?: boolean;
  disabled?: boolean;
};

export function ListRow({ icon: Icon, title, subtitle, value, accessory, onSelect, selected = false, disabled, className, ...rest }: ListRowProps) {
  const body = (
    <>
      {Icon && <span className="ui-list-row__icon" aria-hidden="true"><Icon /></span>}
      <span className="ui-list-row__text">
        <span className="ui-list-row__title">{title}</span>
        {subtitle != null && <span className="ui-list-row__subtitle">{subtitle}</span>}
      </span>
      {value != null && <span className="ui-list-row__value">{value}</span>}
    </>
  );
  return (
    <li {...rest} className={cx("ui-list-row", className)} data-selectable={onSelect ? true : undefined} data-selected={(onSelect && selected) || undefined}>
      {onSelect
        ? <button type="button" className="ui-list-row__main" aria-current={selected ? "true" : undefined} disabled={disabled} onClick={onSelect}>{body}</button>
        : <div className="ui-list-row__main">{body}</div>}
      {accessory != null && <span className="ui-list-row__accessory">{accessory}</span>}
    </li>
  );
}
