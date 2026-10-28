import { ArrowDownRight, ArrowRight, ArrowUpRight } from "lucide-react";
import type { ComponentPropsWithRef, ReactNode } from "react";
import { cx } from "./shared";
import "./StatTile.css";

export type StatTrend = {
  direction: "up" | "down" | "flat";
  /** Short visible text, e.g. "+4%". */
  value?: string;
  /** Full text alternative for assistive tech, e.g. "Up 4% from last week". Required. */
  label: string;
  /** Whether the change is good, bad or neutral; drives color only (the arrow and text carry the meaning). */
  sentiment?: "positive" | "negative" | "neutral";
};

export type StatTileProps = Omit<ComponentPropsWithRef<"dl">, "children"> & {
  label: ReactNode;
  value: ReactNode;
  unit?: ReactNode;
  trend?: StatTrend;
};

const TREND_ICON = { up: ArrowUpRight, down: ArrowDownRight, flat: ArrowRight } as const;

export function StatTile({ label, value, unit, trend, className, ...rest }: StatTileProps) {
  const TrendIcon = trend ? TREND_ICON[trend.direction] : null;
  return (
    <dl {...rest} className={cx("ui-stat", className)}>
      <dt className="ui-stat__label">{label}</dt>
      <dd className="ui-stat__value">
        {value}
        {unit != null && <span className="ui-stat__unit">{unit}</span>}
      </dd>
      {trend && TrendIcon && (
        <dd className="ui-stat__trend" data-direction={trend.direction} data-sentiment={trend.sentiment ?? "neutral"}>
          <TrendIcon className="ui-stat__trend-icon" aria-hidden="true" />
          {trend.value && <span aria-hidden="true">{trend.value}</span>}
          <span className="ui-visually-hidden">{trend.label}</span>
        </dd>
      )}
    </dl>
  );
}
