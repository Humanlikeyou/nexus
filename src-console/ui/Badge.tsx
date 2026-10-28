import type { ComponentPropsWithRef, ReactNode } from "react";
import { cx, type Tone } from "./shared";
import "./Badge.css";

export type BadgeProps = Omit<ComponentPropsWithRef<"span">, "children"> & {
  tone?: Tone;
  /** Required text: color is never the only signal. */
  children: ReactNode;
};

export function Badge({ tone = "neutral", className, children, ...rest }: BadgeProps) {
  return <span {...rest} className={cx("ui-badge", className)} data-tone={tone}>{children}</span>;
}

export type StatusDotProps = Omit<ComponentPropsWithRef<"span">, "children"> & {
  tone?: Tone;
  /** Required text label next to the dot: color is never the only signal. */
  children: ReactNode;
  /** Pulses the dot to show live activity (CSS only; off under reduced motion). */
  pulse?: boolean;
};

export function StatusDot({ tone = "neutral", pulse = false, className, children, ...rest }: StatusDotProps) {
  return (
    <span {...rest} className={cx("ui-status-dot", className)} data-tone={tone} data-pulse={pulse || undefined}>
      <span className="ui-status-dot__dot" aria-hidden="true" />
      <span className="ui-status-dot__label">{children}</span>
    </span>
  );
}
