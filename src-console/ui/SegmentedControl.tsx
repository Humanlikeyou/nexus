import type { LucideIcon } from "lucide-react";
import { useRef, type ComponentPropsWithRef, type KeyboardEvent } from "react";
import { cx } from "./shared";
import "./SegmentedControl.css";

export type SegmentedOption<T extends string> = { value: T; label: string; icon?: LucideIcon };

export type SegmentedControlProps<T extends string> = Omit<ComponentPropsWithRef<"div">, "onChange" | "role" | "children"> & {
  value: T;
  options: ReadonlyArray<SegmentedOption<T>>;
  onChange: (value: T) => void;
  /** Accessible name of the radiogroup. */
  label: string;
  size?: "regular" | "small";
};

const NEXT_KEYS = new Set(["ArrowRight", "ArrowDown"]);
const PREV_KEYS = new Set(["ArrowLeft", "ArrowUp"]);

/** Radiogroup with roving tabindex: Tab enters on the selected segment, arrows move and select. */
export function SegmentedControl<T extends string>({ value, options, onChange, label, size = "regular", className, onKeyDown, ...rest }: SegmentedControlProps<T>) {
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = options.findIndex((option) => option.value === value);
  const focusIndex = selectedIndex === -1 ? 0 : selectedIndex;

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    onKeyDown?.(event);
    if (event.defaultPrevented || options.length === 0) return;
    const current = buttons.current.findIndex((button) => button === event.target);
    if (current === -1) return;
    let next: number;
    if (NEXT_KEYS.has(event.key)) next = (current + 1) % options.length;
    else if (PREV_KEYS.has(event.key)) next = (current - 1 + options.length) % options.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = options.length - 1;
    else return;
    event.preventDefault();
    buttons.current[next]?.focus();
    onChange(options[next].value);
  }

  return (
    // eslint-disable-next-line jsx-a11y/interactive-supports-focus -- roving tabindex: the radios take focus, not the group
    <div {...rest} role="radiogroup" aria-label={label} className={cx("ui-segmented", className)} data-size={size} onKeyDown={handleKeyDown}>
      {options.map((option, index) => {
        const Icon = option.icon;
        const checked = option.value === value;
        return (
          <button
            key={option.value}
            ref={(node) => { buttons.current[index] = node; }}
            type="button"
            role="radio"
            aria-checked={checked}
            tabIndex={index === focusIndex ? 0 : -1}
            className="ui-segmented__segment"
            data-checked={checked || undefined}
            onClick={() => { if (!checked) onChange(option.value); }}
          >
            {Icon && <Icon className="ui-segmented__icon" aria-hidden="true" />}
            <span>{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
