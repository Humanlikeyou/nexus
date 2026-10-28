import type { ComponentPropsWithRef, MouseEvent, ReactNode } from "react";
import { cx } from "./shared";
import "./Toggle.css";

type ToggleBase = Omit<ComponentPropsWithRef<"button">, "children" | "role" | "onChange"> & {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
};

/** A switch needs a name: visible children, or an aria-label / aria-labelledby. */
export type ToggleProps =
  | (ToggleBase & { children: ReactNode })
  | (ToggleBase & { children?: never; "aria-label": string })
  | (ToggleBase & { children?: never; "aria-labelledby": string });

export function Toggle({ checked, onCheckedChange, className, children, type = "button", onClick, ...rest }: ToggleProps) {
  function handleClick(event: MouseEvent<HTMLButtonElement>) {
    onClick?.(event);
    if (!event.defaultPrevented) onCheckedChange(!checked);
  }
  return (
    <button {...rest} type={type} role="switch" aria-checked={checked} className={cx("ui-toggle", className)} data-checked={checked || undefined} onClick={handleClick}>
      {children != null && <span className="ui-toggle__label">{children}</span>}
      <span className="ui-toggle__track" aria-hidden="true"><span className="ui-toggle__thumb" /></span>
    </button>
  );
}
