import { ChevronDown } from "lucide-react";
import { useId, type ComponentPropsWithRef, type ReactNode } from "react";
import { cx, joinIds } from "./shared";
import "./TextField.css";
import "./Select.css";

export type SelectOption = { value: string; label: string; disabled?: boolean };

export type SelectProps = Omit<ComponentPropsWithRef<"select">, "children"> & {
  label: string;
  /** Keeps the label for assistive tech but hides it visually. */
  hideLabel?: boolean;
  description?: ReactNode;
  options: SelectOption[];
};

/** Native <select> styled as a TextField: for 4+ choices where a SegmentedControl won't fit. */
export function Select({ label, hideLabel = false, description, options, className, id, "aria-describedby": describedBy, ...rest }: SelectProps) {
  const autoId = useId();
  const selectId = id ?? `${autoId}-select`;
  const descriptionId = description ? `${autoId}-description` : undefined;
  return (
    <div className={cx("ui-text-field ui-select", className)}>
      <label className={cx("ui-text-field__label", hideLabel && "ui-visually-hidden")} htmlFor={selectId}>{label}</label>
      <div className="ui-text-field__control">
        <select {...rest} id={selectId} className="ui-text-field__input ui-select__input" aria-describedby={joinIds(describedBy, descriptionId)}>
          {options.map((o) => <option key={o.value} value={o.value} disabled={o.disabled}>{o.label}</option>)}
        </select>
        <ChevronDown className="ui-select__chevron" aria-hidden="true" />
      </div>
      {description && <p id={descriptionId} className="ui-text-field__description">{description}</p>}
    </div>
  );
}
