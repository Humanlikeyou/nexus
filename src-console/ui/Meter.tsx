import type { ComponentPropsWithRef, CSSProperties } from "react";
import { cx } from "./shared";
import "./Meter.css";

export type MeterSegment = {
  /** Text label (used in the accessible summary). */
  label: string;
  value: number;
  /** Any CSS color or var(). */
  color?: string;
};

type Base = Omit<ComponentPropsWithRef<"div">, "children" | "color"> & {
  /** Required accessible name. */
  label: string;
  size?: "sm" | "md";
};

export type MeterProps = Base & (
  | { value: number | null | undefined; min?: number; max?: number; valueText?: string; color?: string; segments?: never }
  /** Stacked variant: segments share the bar in proportion to their values (e.g. sleep stages). */
  | { segments: MeterSegment[]; format?: (value: number) => string; value?: never; color?: never }
);

export function Meter(props: MeterProps) {
  const { label, size = "md", className, style, ...restAll } = props;
  if ("segments" in props && props.segments) {
    const { segments, format, ...rest } = restAll as { segments: MeterSegment[]; format?: (v: number) => string };
    const shown = segments.filter((s) => Number.isFinite(s.value) && s.value > 0);
    const total = shown.reduce((sum, s) => sum + s.value, 0);
    const fmt = format ?? ((v: number) => String(Math.round(v)));
    const summary = shown.length ? `${label}: ${shown.map((s) => `${s.label} ${fmt(s.value)}`).join(", ")}` : `${label}: no data`;
    return (
      <div role="img" aria-label={summary} {...rest} className={cx("ui-meter", className)} data-size={size} data-variant="stacked" data-empty={total ? undefined : true} style={style}>
        {shown.map((s) => (
          <span key={s.label} className="ui-meter__segment" style={{ flexGrow: s.value, ...(s.color ? { "--ui-meter-color": s.color } : {}) } as CSSProperties} />
        ))}
      </div>
    );
  }
  const { value, min = 0, max = 100, valueText, color, ...rest } = restAll as { value: number | null | undefined; min?: number; max?: number; valueText?: string; color?: string };
  const has = typeof value === "number" && Number.isFinite(value);
  const clamped = has ? Math.min(max, Math.max(min, value as number)) : min;
  const pct = max > min ? ((clamped - min) / (max - min)) * 100 : 0;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={has ? clamped : undefined}
      aria-valuetext={valueText ?? (has ? undefined : "No data")}
      {...rest}
      className={cx("ui-meter", className)}
      data-size={size}
      data-empty={has ? undefined : true}
      style={{ ...(color ? { "--ui-meter-color": color } : {}), ...style } as CSSProperties}
    >
      <span className="ui-meter__fill" style={{ width: `${pct}%` }} />
    </div>
  );
}
