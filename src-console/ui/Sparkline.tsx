import type { ComponentPropsWithRef, CSSProperties } from "react";
import { cx } from "./shared";
import "./Sparkline.css";

export type SparklineProps = Omit<ComponentPropsWithRef<"svg">, "children" | "color" | "values" | "format"> & {
  /** Series in order; null / non-finite values are gaps, never zeros. */
  values: Array<number | null | undefined>;
  /** Required accessible name; a min/max/last summary is appended. */
  label: string;
  format?: (value: number) => string;
  /** Draws a dashed line at the mean of the non-null values. */
  showMean?: boolean;
  color?: string;
  size?: "sm" | "lg";
};

const W = 100;
const H = 32;
const PAD = 2;

function finite(v: number | null | undefined): v is number {
  return typeof v === "number" && Number.isFinite(v);
}

export function sparklineSummary(values: Array<number | null | undefined>, format: (v: number) => string = (v) => String(Math.round(v * 10) / 10)) {
  const nums = values.filter(finite);
  if (nums.length === 0) return "no data";
  const last = [...values].reverse().find(finite) as number;
  return `min ${format(Math.min(...nums))}, max ${format(Math.max(...nums))}, last ${format(last)}`;
}

export function Sparkline({ values, label, format, showMean = false, color, size = "sm", className, style, ...rest }: SparklineProps) {
  const nums = values.filter(finite);
  const lo = nums.length ? Math.min(...nums) : 0;
  const hi = nums.length ? Math.max(...nums) : 1;
  const span = hi - lo || 1;
  const n = values.length;
  const x = (i: number) => (n <= 1 ? W / 2 : (i / (n - 1)) * W);
  const y = (v: number) => H - PAD - ((v - lo) / span) * (H - PAD * 2);
  // One "M…L…" run per contiguous stretch of non-null points: nulls become gaps.
  let d = "";
  let open = false;
  const dots: Array<[number, number]> = [];
  values.forEach((v, i) => {
    if (!finite(v)) { open = false; return; }
    const prevOk = i > 0 && finite(values[i - 1]);
    const nextOk = i < n - 1 && finite(values[i + 1]);
    if (!prevOk && !nextOk) dots.push([x(i), y(v)]);
    d += `${open ? "L" : "M"}${x(i).toFixed(2)} ${y(v).toFixed(2)} `;
    open = true;
  });
  const mean = nums.length ? nums.reduce((a, b) => a + b, 0) / nums.length : null;
  return (
    <svg
      role="img"
      aria-label={`${label}: ${sparklineSummary(values, format)}`}
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio="none"
      {...rest}
      className={cx("ui-sparkline", className)}
      data-size={size}
      data-empty={nums.length ? undefined : true}
      style={{ ...(color ? { "--ui-spark-color": color } : {}), ...style } as CSSProperties}
    >
      {showMean && mean != null && <line className="ui-sparkline__mean" x1="0" x2={W} y1={y(mean)} y2={y(mean)} data-testid="sparkline-mean" />}
      {d && <path className="ui-sparkline__line" d={d.trim()} />}
      {dots.map(([cx0, cy0]) => <circle key={`${cx0}-${cy0}`} className="ui-sparkline__dot" cx={cx0} cy={cy0} r="1.5" />)}
    </svg>
  );
}
