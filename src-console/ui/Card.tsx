import { useId, type ComponentPropsWithoutRef, type ReactNode, type Ref } from "react";
import { cx } from "./shared";
import "./Card.css";

export type CardProps = Omit<ComponentPropsWithoutRef<"section">, "title"> & {
  ref?: Ref<HTMLElement>;
  as?: "section" | "article" | "aside" | "div";
  /** Rendered as a heading; the card is then labelled by it. */
  title?: ReactNode;
  headingLevel?: 2 | 3 | 4;
  /** Trailing header content (e.g. a Button or Badge). */
  accessory?: ReactNode;
  padding?: "md" | "none";
  elevation?: 0 | 1 | 2;
};

export function Card({ as = "section", title, headingLevel = 2, accessory, padding = "md", elevation = 1, className, children, ...rest }: CardProps) {
  // Every allowed tag is a plain HTMLElement; typing it as one lets Ref<HTMLElement> through.
  const Tag = as as "section";
  const headingId = useId();
  const Heading = `h${headingLevel}` as const;
  const hasTitle = title != null && title !== false;
  return (
    <Tag
      aria-labelledby={hasTitle ? headingId : undefined}
      {...rest}
      className={cx("ui-card", className)}
      data-padding={padding}
      data-elevation={elevation}
    >
      {(hasTitle || accessory) && (
        <header className="ui-card__header">
          {hasTitle && <Heading id={headingId} className="ui-card__title">{title}</Heading>}
          {accessory && <div className="ui-card__accessory">{accessory}</div>}
        </header>
      )}
      {children}
    </Tag>
  );
}
