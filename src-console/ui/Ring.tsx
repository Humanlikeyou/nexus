import type { ComponentPropsWithRef, CSSProperties, ReactNode } from "react";
import { cx } from "./shared";
import "./Ring.css";

export type RingProps = Omit<ComponentPropsWithRef<"div">, "children" | "color"> & {
  /** Current value; null renders the track only (no data yet). */
  value: number | null | undefined;
  min?: number;
  max?: number;
  /** Required accessible name, e.g. "Recovery". */
  label: string;
  /** Optional spoken value, e.g. "74%, High". */
  valueText?: string;
  /** Stroke color, any CSS color or var(), e.g. "var(--data-strain)". Defaults to --accent. */
  color?: string;
  size?: "sm" | "md" | "lg";
  /** Center content (readout); decorative ring stays aria-hidden. */
  children?: ReactNode;
};

const R = 42;
const C = 2 * Math.PI * R;

export function Ring({ value, min = 0, max = 100, label, valueText, color, size = "md", className, style, children, ...rest }: RingProps) {
  const has = typeof value === "number" && Number.isFinite(value);
  const clamped = has ? Math.min(max, Math.max(min, value as number)) : min;
  const fraction = max > min ? (clamped - min) / (max - min) : 0;
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={has ? clamped : undefined}
      aria-valuetext={valueText ?? (has ? undefined : "No data")}
      {...rest}
      className={cx("ui-ring", className)}
      data-size={size}
      data-empty={has ? undefined : true}
      style={{ ...(color ? { "--ui-ring-color": color } : {}), ...style } as CSSProperties}
    >
      <svg className="ui-ring__svg" viewBox="0 0 100 100" aria-hidden="true">
        <circle className="ui-ring__track" cx="50" cy="50" r={R} />
        {has && <circle className="ui-ring__fill" cx="50" cy="50" r={R} strokeDasharray={C} strokeDashoffset={C * (1 - fraction)} />}
      </svg>
      {children != null && <div className="ui-ring__center">{children}</div>}
    </div>
  );
}
